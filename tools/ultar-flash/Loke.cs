// Ultar Flash – eigener Schreibweg für Samsung-Geräte im Download-Modus (Odin/Loke-Protokoll).
// Spricht das Handy über die serielle Schnittstelle an, die Windows für den Download-Modus anlegt (COM-Port) –
// dafür ist kein heimdall und kein Treibertausch nötig. Ablauf in EINER Sitzung:
//   Begrüßung -> Sitzung -> Partitionstabelle lesen (noch nichts geschrieben) -> prüfen -> Abbilder senden -> Neustart.
// Protokoll nach der offenen Beschreibung aus dem Heimdall-Projekt. EXPERIMENTELL: an echten Geräten noch nicht erprobt.
using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Ports;
using System.Linq;
using System.Text;

namespace UltarFlash
{
    public class PitEntry { public uint BinaryType, DeviceType, Id; public string Name = "", FlashName = ""; }

    public class Loke : IDisposable
    {
        readonly Stream io;
        readonly IDisposable owner;
        int packetSize = 131072, seqMax = 800;
        /// true, sobald das erste Abbild angefangen wurde – vorher ist am Handy garantiert nichts verändert.
        public bool WroteAnything;

        public Loke(Stream s, IDisposable owner) { io = s; this.owner = owner; }
        public void Dispose() { try { io.Dispose(); } catch { } try { if (owner != null) owner.Dispose(); } catch { } }

        void Send(byte[] b) { io.Write(b, 0, b.Length); io.Flush(); }

        byte[] Recv(int n)
        {
            var b = new byte[n]; int got = 0;
            while (got < n)
            {
                int r = io.Read(b, got, n - got);
                if (r <= 0) throw new Exception("Das Handy antwortet nicht mehr (Verbindung im Download-Modus abgebrochen).");
                got += r;
            }
            return b;
        }

        static byte[] Ctl(uint type, uint request, params uint[] args)
        {
            var p = new byte[1024];
            BitConverter.GetBytes(type).CopyTo(p, 0); BitConverter.GetBytes(request).CopyTo(p, 4);
            for (int i = 0; i < args.Length; i++) BitConverter.GetBytes(args[i]).CopyTo(p, 8 + 4 * i);
            return p;
        }

        /// Steuerpaket senden, 8-Byte-Antwort lesen; liefert den Ergebniswert.
        uint Cmd(uint type, uint request, string what, params uint[] args)
        {
            Send(Ctl(type, request, args));
            var r = Recv(8);
            uint rt = BitConverter.ToUInt32(r, 0), res = BitConverter.ToUInt32(r, 4);
            if (rt != type) throw new Exception("Unerwartete Antwort des Handys bei \"" + what + "\" (Typ " + rt + " statt " + type + ").");
            return res;
        }

        public void Handshake()
        {
            Send(Encoding.ASCII.GetBytes("ODIN"));
            var r = Encoding.ASCII.GetString(Recv(4));
            if (r != "LOKE") throw new Exception("Das Handy hat die Begrüßung nicht erwidert (\"" + r + "\" statt LOKE).");
        }

        public void BeginSession()
        {
            uint def = Cmd(0x64, 0, "Sitzung beginnen", 4);
            if (def != 0)
            {
                // Gerät kann große Pakete: 1 MiB aushandeln
                if (Cmd(0x64, 5, "Paketgröße", 1048576) != 0) throw new Exception("Das Handy lehnt die Paketgröße ab.");
                packetSize = 1048576; seqMax = 30;
            }
        }

        public List<PitEntry> ReadPit()
        {
            uint size = Cmd(0x65, 1, "Partitionstabelle anfordern");
            if (size < 28 || size > 1 << 20) throw new Exception("Unplausible Größe der Partitionstabelle (" + size + ").");
            var pit = new byte[size];
            for (uint part = 0, off = 0; off < size; part++)
            {
                Send(Ctl(0x65, 2, part));
                int n = (int)Math.Min(500, size - off);
                Recv(n).CopyTo(pit, off); off += (uint)n;
            }
            Cmd(0x65, 3, "Partitionstabelle abschließen");
            if (BitConverter.ToUInt32(pit, 0) != 0x12349876) throw new Exception("Partitionstabelle hat nicht das erwartete Format.");
            int count = (int)BitConverter.ToUInt32(pit, 4);
            var list = new List<PitEntry>();
            for (int i = 0; i < count; i++)
            {
                int o = 28 + i * 132;
                if (o + 132 > pit.Length) break;
                Func<int, string> str = so => { var raw = Encoding.ASCII.GetString(pit, o + so, 32); int z = raw.IndexOf('\0'); return z >= 0 ? raw.Substring(0, z) : raw; };
                list.Add(new PitEntry { BinaryType = BitConverter.ToUInt32(pit, o), DeviceType = BitConverter.ToUInt32(pit, o + 4), Id = BitConverter.ToUInt32(pit, o + 8), Name = str(36), FlashName = str(68) });
            }
            return list;
        }

        public void TotalBytes(long total)
        {
            if (Cmd(0x64, 2, "Gesamtgröße", (uint)(total & 0xFFFFFFFF), (uint)(total >> 32)) != 0) throw new Exception("Das Handy lehnt die Gesamtgröße ab.");
        }

        public void SendFile(string file, PitEntry e, Action<int> progress)
        {
            using (var f = File.OpenRead(file))
            {
                long size = f.Length, sent = 0;
                if (Cmd(0x66, 0, "Übertragung beginnen") != 0) throw new Exception("Das Handy lehnt die Übertragung ab.");
                WroteAnything = true;
                long seqBytes = (long)seqMax * packetSize;
                var buf = new byte[packetSize];
                while (sent < size)
                {
                    long thisSeq = Math.Min(seqBytes, size - sent);
                    int parts = (int)((thisSeq + packetSize - 1) / packetSize);
                    if (Cmd(0x66, 2, "Abschnitt beginnen", (uint)(parts * (long)packetSize)) != 0) throw new Exception("Das Handy lehnt den Abschnitt ab.");
                    for (int p = 0; p < parts; p++)
                    {
                        Array.Clear(buf, 0, buf.Length);
                        int want = (int)Math.Min(packetSize, size - sent - (long)p * packetSize), got = 0;
                        while (got < want) { int r = f.Read(buf, got, want - got); if (r <= 0) throw new Exception("Lesefehler in " + file); got += r; }
                        Send(buf); // letzter Teil mit Nullen auf die Paketgröße aufgefüllt
                        var resp = Recv(8);
                        if (BitConverter.ToUInt32(resp, 0) != 0 || BitConverter.ToUInt32(resp, 4) != (uint)p) throw new Exception("Das Handy hat Teil " + p + " von " + e.Name + " nicht bestätigt.");
                        progress((int)(100 * Math.Min(size, sent + (long)(p + 1) * packetSize) / size));
                    }
                    bool last = sent + thisSeq >= size;
                    // Abschluss des Abschnitts: Ziel (0 = Hauptprozessor), echte Bytezahl, Gerätetyp und Partitionsnummer aus der Tabelle
                    if (Cmd(0x66, 3, "Abschnitt abschließen", e.BinaryType, (uint)thisSeq, 0, e.DeviceType, e.Id, last ? 1u : 0u) != 0) throw new Exception("Das Handy meldet einen Fehler beim Schreiben von " + e.Name + ".");
                    sent += thisSeq;
                }
            }
        }

        public void EndSession(bool reboot)
        {
            // Manche Geräte beantworten „Sitzung beenden“ unerwartet – den Neustart trotzdem versuchen
            Exception first = null;
            try { Cmd(0x67, 0, "Sitzung beenden"); } catch (Exception e) { first = e; }
            if (reboot) try { Cmd(0x67, 1, "Neustart"); } catch (Exception e) { if (first == null) first = e; }
            if (first != null) throw first;
        }

        // ---------------- Verbindung ----------------

        /// Letzte Meldungen der Port-Suche (für das Protokoll, wenn kein Gerät gefunden wird)
        public static string LastProbe = "";

        /// COM-Ports, an denen ein Samsung-Gerät hängen kann. Samsung meldet den Download-Modus als MODEM an –
        /// dessen Port steht nicht im Gerätenamen, sondern in den Modem-Daten bzw. in der Registrierung.
        public static List<string> SamsungPorts()
        {
            var res = new List<string>();
            var forced = Environment.GetEnvironmentVariable("ULTARFLASH_COM");
            if (!string.IsNullOrEmpty(forced)) { res.Add(forced); return res; }
            try
            {   // 1. Modems (so erscheint der Download-Modus mit dem Samsung-Treiber)
                using (var s = new System.Management.ManagementObjectSearcher("SELECT AttachedTo, PNPDeviceID, Name FROM Win32_POTSModem"))
                    foreach (System.Management.ManagementObject o in s.Get())
                    {
                        string id = (o["PNPDeviceID"] ?? "").ToString().ToUpper(), name = (o["Name"] ?? "").ToString().ToUpper(), port = (o["AttachedTo"] ?? "").ToString();
                        if (port.StartsWith("COM") && (id.Contains("VID_04E8") || name.Contains("SAMSUNG"))) res.Add(port);
                    }
            }
            catch { }
            try
            {   // 2. Alle Samsung-USB-Geräte: Port aus dem Namen "(COMx)" oder aus der Registrierung (PortName)
                using (var s = new System.Management.ManagementObjectSearcher("SELECT Name, DeviceID FROM Win32_PnPEntity WHERE DeviceID LIKE '%VID_04E8%'"))
                    foreach (System.Management.ManagementObject o in s.Get())
                    {
                        var name = (o["Name"] ?? "").ToString();
                        int a = name.LastIndexOf("(COM"), b = name.LastIndexOf(')');
                        if (a >= 0 && b > a) res.Add(name.Substring(a + 1, b - a - 1));
                        try
                        {
                            var pn = Microsoft.Win32.Registry.GetValue(@"HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Enum\" + o["DeviceID"] + @"\Device Parameters", "PortName", null) as string;
                            if (!string.IsNullOrEmpty(pn) && pn.StartsWith("COM")) res.Add(pn);
                        }
                        catch { }
                    }
            }
            catch { }
            res = res.Distinct().ToList();
            // 3. Nichts Eindeutiges gefunden: alle vorhandenen Ports versuchen (die Begrüßung entscheidet)
            if (res.Count == 0) try { res.AddRange(SerialPort.GetPortNames().Distinct()); } catch { }
            return res;
        }

        /// Verbindung zum Handy im Download-Modus öffnen und begrüßen. null, wenn (noch) kein Gerät da ist.
        public static Loke Open(Action<string> log)
        {
            var tcp = Environment.GetEnvironmentVariable("ULTARFLASH_LOKE_TCP"); // nur für Tests: Geräte-Attrappe über TCP
            if (tcp != null)
            {
                try
                {
                    var c = new System.Net.Sockets.TcpClient(tcp.Split(new[] { ':' })[0], int.Parse(tcp.Split(new[] { ':' })[1]));
                    c.ReceiveTimeout = 15000;
                    var l = new Loke(c.GetStream(), c); l.Handshake(); return l;
                }
                catch { return null; }
            }
            var probe = new StringBuilder();
            foreach (var port in SamsungPorts())
            {
                SerialPort sp = null;
                try
                {
                    sp = new SerialPort(port, 115200) { ReadTimeout = 3000, WriteTimeout = 5000, DtrEnable = true, RtsEnable = true };
                    sp.Open();
                    var l = new Loke(sp.BaseStream, sp);
                    l.Handshake();
                    sp.ReadTimeout = 180000; // Schreiben großer Abschnitte kann dauern
                    log("Download-Modus gefunden an " + port + ".");
                    return l;
                }
                catch (Exception e)
                {
                    probe.Append(port + ": " + (e is UnauthorizedAccessException ? "belegt (läuft Odin oder ein anderes Programm? Bitte schließen)" : e.Message) + "; ");
                    try { if (sp != null) sp.Dispose(); } catch { }
                }
            }
            LastProbe = probe.Length > 0 ? probe.ToString() : "keine COM-Ports vorhanden";
            return null;
        }

        /// Kompletter Schreibvorgang. Liefert true bei Erfolg. Wirft nicht; meldet über log.
        public static bool Flash(Dictionary<string, string> images, int waitSeconds, Action<string> log, Action<int> progress)
        {
            Loke l = null;
            for (int i = 0; i < waitSeconds && l == null; i++) { l = Open(log); if (l == null) System.Threading.Thread.Sleep(1000); }
            if (l == null) { log("Kein Samsung-Gerät im Download-Modus gefunden (kein passender COM-Port)."); log("Versuchte Anschlüsse: " + LastProbe); return false; }
            using (l)
            {
                try
                {
                    l.BeginSession();
                    var pit = l.ReadPit();
                    log("Partitionstabelle gelesen: " + pit.Count + " Einträge.");
                    var plan = new List<KeyValuePair<PitEntry, string>>();
                    foreach (var kv in images)
                    {
                        var e = pit.FirstOrDefault(x => x.Name.ToLower() == kv.Key);
                        if (e == null || !Core.Writable.Contains(e.Name.ToLower()))
                        {
                            log("Abbruch: Partition " + kv.Key + " steht nicht in der Partitionstabelle des Geräts.");
                            l.EndSession(true);
                            return false;
                        }
                        plan.Add(new KeyValuePair<PitEntry, string>(e, kv.Value));
                    }
                    l.TotalBytes(plan.Sum(p => new FileInfo(p.Value).Length));
                    foreach (var p in plan)
                    {
                        log("Schreibe " + p.Key.Name + " …");
                        l.SendFile(p.Value, p.Key, progress);
                    }
                    // Alle Abbilder sind vom Handy bestätigt. Der Abschluss (Sitzung beenden, Neustart) ist nur noch Formsache:
                    // Antwortet das Handy dabei unerwartet, gilt das Schreiben trotzdem als gelungen.
                    try { l.EndSession(true); }
                    catch (Exception ex)
                    {
                        // Samsung prüft die Abbilder erst beim Abschluss. Lehnt das Handy dort ab, wurde NICHTS übernommen.
                        log("Das Handy hat die Abbilder beim Abschluss abgelehnt: " + ex.Message);
                        log("Steht am Handy oben „Only official released binaries are allowed to be flashed“, sperrt Samsung eigene Abbilder noch (KG STATE: Prenormal).");
                        log("Abhilfe: Android starten, mit dem Internet verbinden und warten, bis in den Entwickleroptionen „OEM-Entsperrung“ erscheint; im Download-Modus muss „KG STATE: Checking“ stehen.");
                        log("Handy jetzt von Hand neu starten: Leiser + Ein/Aus etwa 7 Sekunden halten.");
                        return false;
                    }
                    return true;
                }
                catch (Exception ex)
                {
                    log("Fehler: " + ex.Message);
                    log(l.WroteAnything ? "Der Schreibvorgang wurde unterbrochen. Das Handy kann jetzt möglicherweise nicht starten – über den Download-Modus wiederherstellen." : "Es wurde nichts geschrieben.");
                    return false;
                }
            }
        }
    }
}
