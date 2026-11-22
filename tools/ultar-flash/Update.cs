// Ultar Flash – Aktualisierung aus den GitHub-Releases des Projekts.
// Holt das neueste Release, lädt die MSI-Datei, prüft sie gegen SHA256SUMS und startet die Installation.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net;
using System.Web.Script.Serialization;

namespace UltarFlash
{
    public class ReleaseInfo { public string Tag = "", MsiName = "", MsiUrl = "", SumsUrl = ""; public bool Newer; }

    public static class Updater
    {
        public const string Version = "0.6.0";
        public const string Repo = "Anas-Karah-th-ab-de/ultar-os";

        static string Api { get { return Environment.GetEnvironmentVariable("ULTARFLASH_API") ?? "https://api.github.com"; } }

        public static string SettingsDir { get { var d = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "UltarFlash"); Directory.CreateDirectory(d); return d; } }

        /// Zugangs-Token für das (private) GitHub-Projekt; liegt nur im Benutzerprofil.
        public static string Token
        {
            get { var f = Path.Combine(SettingsDir, "token.txt"); return File.Exists(f) ? File.ReadAllText(f).Trim() : ""; }
            set { File.WriteAllText(Path.Combine(SettingsDir, "token.txt"), value.Trim()); }
        }

        static HttpWebRequest Req(string url, string accept)
        {
            ServicePointManager.SecurityProtocol |= (SecurityProtocolType)3072; // TLS 1.2
            var r = (HttpWebRequest)WebRequest.Create(url);
            r.UserAgent = "UltarFlash/" + Version; r.Accept = accept; r.Timeout = 30000; r.ReadWriteTimeout = 300000;
            // Token nur an die GitHub-Schnittstelle selbst senden, nie an Weiterleitungsziele
            if (Token.Length > 0 && url.StartsWith(Api)) r.Headers["Authorization"] = "Bearer " + Token;
            return r;
        }

        public static int Cmp(string a, string b)
        {
            Version va, vb;
            if (!System.Version.TryParse(a.TrimStart('v', 'V'), out va) || !System.Version.TryParse(b.TrimStart('v', 'V'), out vb)) return string.CompareOrdinal(a, b);
            return va.CompareTo(vb);
        }

        public static ReleaseInfo Check()
        {
            string json;
            using (var resp = Req(Api + "/repos/" + Repo + "/releases/latest", "application/vnd.github+json").GetResponse())
            using (var sr = new StreamReader(resp.GetResponseStream())) json = sr.ReadToEnd();
            var o = (Dictionary<string, object>)new JavaScriptSerializer { MaxJsonLength = 1 << 24 }.DeserializeObject(json);
            var info = new ReleaseInfo { Tag = (o["tag_name"] ?? "").ToString() };
            foreach (Dictionary<string, object> a in (object[])o["assets"])
            {
                string name = a["name"].ToString(), url = a["url"].ToString();
                if (name.EndsWith(".msi", StringComparison.OrdinalIgnoreCase)) { info.MsiName = name; info.MsiUrl = url; }
                if (name == "SHA256SUMS") info.SumsUrl = url;
            }
            info.Newer = Cmp(info.Tag, Version) > 0;
            return info;
        }

        static void Fetch(string url, string file)
        {
            using (var resp = Req(url, "application/octet-stream").GetResponse())
            using (var s = resp.GetResponseStream()) using (var f = File.Create(file)) s.CopyTo(f, 1 << 16);
        }

        /// MSI laden und prüfen. Liefert den Pfad der geprüften Datei; wirft bei fehlender oder falscher Prüfsumme.
        public static string Download(ReleaseInfo r, Action<string> log)
        {
            if (r.MsiUrl.Length == 0) throw new Exception("Das Release enthält keine MSI-Datei.");
            if (r.SumsUrl.Length == 0) throw new Exception("Das Release enthält keine SHA256SUMS – Installation abgelehnt.");
            var dir = Path.Combine(Path.GetTempPath(), "UltarFlash-Update"); Directory.CreateDirectory(dir);
            string msi = Path.Combine(dir, r.MsiName), sums = Path.Combine(dir, "SHA256SUMS");
            log("Lade " + r.MsiName + " …");
            Fetch(r.SumsUrl, sums); Fetch(r.MsiUrl, msi);
            var want = File.ReadAllLines(sums).Select(l => l.Split(new[] { ' ', '*' }, StringSplitOptions.RemoveEmptyEntries)).Where(t => t.Length == 2 && t[1] == r.MsiName).Select(t => t[0].ToLower()).FirstOrDefault();
            if (want == null) throw new Exception("SHA256SUMS enthält keinen Eintrag für " + r.MsiName + ".");
            if (want != Core.Sha256(msi)) { File.Delete(msi); throw new Exception("Prüfsumme der MSI-Datei stimmt nicht – Installation abgelehnt."); }
            log("Prüfsumme stimmt.");
            return msi;
        }

        public static void Install(string msi)
        {
            System.Diagnostics.Process.Start("msiexec.exe", "/i \"" + msi + "\"");
        }
    }
}
