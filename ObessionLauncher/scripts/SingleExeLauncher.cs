using System;
using System.Diagnostics;
using System.IO;
using System.IO.Compression;
using System.Reflection;
using System.Threading;
using System.Windows.Forms;

namespace ObsessionLauncher
{
    class Program
    {
        [STAThread]
        static void Main(string[] args)
        {
            try
            {
                string extractDir = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                    "VlarpSu", "Obsession");

                Directory.CreateDirectory(extractDir);
                CleanDirectory(extractDir);

                string zipPath = Path.Combine(extractDir, "obsession.zip");
                ExtractEmbeddedZip(zipPath);
                ZipFile.ExtractToDirectory(zipPath, extractDir);
                File.Delete(zipPath);

                string appPath = Path.Combine(extractDir, "obsession.exe");
                if (!File.Exists(appPath))
                {
                    throw new FileNotFoundException("obsession.exe not found after extraction", appPath);
                }

                var psi = new ProcessStartInfo(appPath);
                psi.WorkingDirectory = extractDir;
                if (args.Length > 0)
                {
                    psi.Arguments = string.Join(" ", args);
                }
                Process.Start(psi);
            }
            catch (Exception ex)
            {
                var errorPath = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                    "VlarpSu", "ObsessionLauncherError.txt");
                Directory.CreateDirectory(Path.GetDirectoryName(errorPath));
                File.WriteAllText(errorPath, ex.ToString());
                MessageBox.Show(
                    "Failed to launch Obsession:\n" + ex.Message + "\n\nDetails: " + errorPath,
                    "Obsession Launcher Error",
                    MessageBoxButtons.OK,
                    MessageBoxIcon.Error);
            }
        }

        static void ExtractEmbeddedZip(string destinationPath)
        {
            var assembly = Assembly.GetExecutingAssembly();
            using (var stream = assembly.GetManifestResourceStream("obsession.zip"))
            {
                if (stream == null)
                {
                    throw new InvalidOperationException("Embedded resource 'obsession.zip' not found.");
                }
                using (var file = File.Create(destinationPath))
                {
                    stream.CopyTo(file);
                }
            }
        }

        static void CleanDirectory(string path)
        {
            foreach (var file in Directory.GetFiles(path))
            {
                try { File.Delete(file); } catch { }
            }
            foreach (var dir in Directory.GetDirectories(path))
            {
                try { Directory.Delete(dir, true); } catch { }
            }
        }
    }
}
