// Diagnose: Liest den Bericht, den Ultar OS über USB als serielle Schnittstelle „Ultar OS“ (VID 1209, PID 0001) sendet.
// Damit lässt sich auch bei dunklem Bildschirm feststellen, ob Ultar OS auf dem Handy läuft und wo es hängt.
using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Ports;
using System.Linq;
using System.Text;

namespace UltarFlash
{
    public static class Diag
    {
        const string Begin = "ULTAR-DIAGNOSE ANFANG", End = "ULTAR-DIAGNOSE ENDE";

        public static List<string> Ports()
        {
            var res = new List<string>();
            var forced = Environment.GetEnvironmentVariable("ULTARFLASH_DIAG_COM");
            if (!string.IsNullOrEmpty(forced)) { res.Add(forced); return res; }
            try
            {
                using (var s = new System.Management.ManagementObjectSearcher("SELECT Name, DeviceID FROM Win32_PnPEntity WHERE DeviceID LIKE '%VID_1209&PID_0001%'"))
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
            return res.Distinct().ToList();
        }

        static string ReadReport(Stream st, int seconds)
        {
            var sb = new StringBuilder(); var buf = new byte[4096]; var until = DateTime.UtcNow.AddSeconds(seconds);
            while (DateTime.UtcNow < until)
            {
                int n;
                try { n = st.Read(buf, 0, buf.Length); } catch (TimeoutException) { continue; } catch (IOException) { break; }
                if (n <= 0) { System.Threading.Thread.Sleep(100); continue; }
                sb.Append(Encoding.UTF8.GetString(buf, 0, n));
                var t = sb.ToString(); int a = t.IndexOf(Begin);
                if (a >= 0) { int e = t.IndexOf(End, a); if (e > a) return t.Substring(a, e - a + End.Length); }
            }
            return sb.Length > 0 ? sb.ToString() : null; // unvollständig: trotzdem zeigen
        }

        /// Wartet bis zu waitSeconds auf Ultar OS am USB und speichert den Bericht. Liefert den Dateipfad oder null.
        public static string Read(string outDir, int waitSeconds, Action<string> log)
        {
            string text = null;
            var tcp = Environment.GetEnvironmentVariable("ULTARFLASH_DIAG_TCP"); // nur für Tests
            if (tcp != null)
            {
                try { var p = tcp.Split(new[] { ':' }); using (var c = new System.Net.Sockets.TcpClient(p[0], int.Parse(p[1]))) { c.ReceiveTimeout = 1000; text = ReadReport(c.GetStream(), 10); } } catch { }
            }
            else
            {
                log("Suche Ultar OS am USB (serielle Schnittstelle „Ultar OS“) …");
                for (int i = 0; i < waitSeconds && text == null; i++)
                {
                    foreach (var port in Ports())
                    {
                        try
                        {
                            using (var sp = new SerialPort(port, 115200) { ReadTimeout = 1000, DtrEnable = true, RtsEnable = true })
                            {
                                sp.Open();
                                log("Ultar OS gefunden an " + port + ". Lese Diagnose …");
                                text = ReadReport(sp.BaseStream, 15);
                            }
                        }
                        catch (Exception e) { log(port + ": " + e.Message); }
                        if (text != null) break;
                    }
                    if (text == null) System.Threading.Thread.Sleep(1000);
                }
            }
            if (text == null)
            {
                log("Ultar OS meldet sich nicht am USB.");
                log("Das heißt: Ultar OS läuft auf dem Handy nicht (oder dessen USB-Anschluss wurde nicht eingeschaltet).");
                return null;
            }
            Directory.CreateDirectory(outDir);
            var file = Path.Combine(outDir, "diagnose-" + DateTime.Now.ToString("yyyyMMdd-HHmmss") + ".txt");
            File.WriteAllText(file, text.Replace("\r\n", "\n").Replace("\n", "\r\n"), new UTF8Encoding(false));
            log("Ultar OS läuft auf dem Handy. Diagnose gespeichert: " + file);
            foreach (var l in text.Replace("\r", "").Split(new[] { '\n' }).Where(x => x.StartsWith("version:") || x.StartsWith("uptime:") || x.StartsWith("usb:") || x.StartsWith("fb0:") || x.StartsWith("dev/dri:") || x.StartsWith("dev/input:")))
                log("  " + l);
            return file;
        }
    }
}
