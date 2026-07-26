use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

pub struct TestInterface {
    devcon: PathBuf,
    device_instance_id: String,
    name: String,
}

impl TestInterface {
    pub fn install_disabled() -> Self {
        let devcon = find_devcon();
        let existing = km_test_loopback_adapters()
            .into_iter()
            .map(|adapter| adapter.device_instance_id)
            .collect::<HashSet<_>>();
        let windows_dir =
            std::env::var_os("WINDIR").expect("WINDIR should be set on Windows runners");
        let netloop_inf = PathBuf::from(windows_dir).join("INF").join("netloop.inf");

        run_command(
            Command::new(&devcon).args([
                "install".as_ref(),
                netloop_inf.as_os_str(),
                "*MSLOOP".as_ref(),
            ]),
            "install Microsoft KM-TEST Loopback Adapter",
        );

        let deadline = Instant::now() + Duration::from_secs(10);
        let adapter = loop {
            if let Some(adapter) = km_test_loopback_adapters()
                .into_iter()
                .find(|adapter| !existing.contains(&adapter.device_instance_id))
            {
                break adapter;
            }
            assert!(
                Instant::now() < deadline,
                "timed out discovering newly installed Microsoft KM-TEST Loopback Adapter"
            );
            thread::sleep(Duration::from_millis(100));
        };

        let name = format!("netwatcher-test-{}", std::process::id());
        let test_interface = Self {
            devcon,
            device_instance_id: adapter.device_instance_id,
            name,
        };
        test_interface.configure_disabled(adapter.interface_index);
        test_interface
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn enable(&self) {
        run_powershell(&format!(
            "Enable-NetAdapter -Name '{}' -Confirm:$false -ErrorAction Stop",
            self.name
        ));
    }

    pub fn disable(&self) {
        run_powershell(&format!(
            "Disable-NetAdapter -Name '{}' -Confirm:$false -ErrorAction Stop",
            self.name
        ));
    }

    fn configure_disabled(&self, interface_index: u32) {
        run_powershell(&format!(
            "\
$adapter = Get-NetAdapter -InterfaceIndex {interface_index} -IncludeHidden -ErrorAction Stop
$adapter | Rename-NetAdapter -NewName '{}' -Confirm:$false -ErrorAction Stop
Disable-NetAdapterBinding -Name '{}' -ComponentID ms_tcpip6 -Confirm:$false -ErrorAction Stop
Set-NetIPInterface -InterfaceAlias '{}' -AddressFamily IPv4 -Dhcp Disabled -ErrorAction Stop
Get-NetIPAddress -InterfaceAlias '{}' -AddressFamily IPv4 -ErrorAction SilentlyContinue |
    Remove-NetIPAddress -Confirm:$false -ErrorAction Stop
Disable-NetAdapter -Name '{}' -Confirm:$false -ErrorAction Stop
",
            self.name, self.name, self.name, self.name, self.name
        ));
    }
}

impl Drop for TestInterface {
    fn drop(&mut self) {
        let _ = Command::new(&self.devcon)
            .args(["remove", &format!("@{}", self.device_instance_id)])
            .output();
    }
}

struct AdapterInfo {
    device_instance_id: String,
    interface_index: u32,
}

fn find_devcon() -> PathBuf {
    let output = run_powershell(
        "\
$devcon = Get-Command devcon.exe, devcon64.exe -ErrorAction SilentlyContinue |
    Select-Object -First 1 -ExpandProperty Source
$chocolateyPackage = Join-Path $env:ChocolateyInstall 'lib\\devcon.portable'
if (-not $devcon) {
    $devcon = Get-ChildItem $chocolateyPackage -Filter devcon64.exe -Recurse -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty FullName
}
$tools = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\\10\\Tools'
if (-not $devcon) {
    $devcon = Get-ChildItem $tools -Filter devcon.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\\\x64\\\\devcon\\.exe$' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $devcon) {
    throw 'Could not find DevCon on PATH, in its Chocolatey package, or below Windows Kits'
}
$devcon
",
    );
    PathBuf::from(output.trim())
}

fn km_test_loopback_adapters() -> Vec<AdapterInfo> {
    let output = run_powershell(
        "\
Get-NetAdapter -IncludeHidden |
    Where-Object { $_.DriverDescription -eq 'Microsoft KM-TEST Loopback Adapter' } |
    ForEach-Object { \"$($_.PnPDeviceID)|$($_.ifIndex)\" }
",
    );
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (device_instance_id, interface_index) = line
                .trim()
                .split_once('|')
                .expect("unexpected Get-NetAdapter output for KM-TEST adapter");
            AdapterInfo {
                device_instance_id: device_instance_id.to_owned(),
                interface_index: interface_index
                    .parse()
                    .expect("KM-TEST adapter should have a numeric interface index"),
            }
        })
        .collect()
}

fn run_powershell(script: &str) -> String {
    run_command(
        Command::new("powershell").args(["-NoProfile", "-Command", script]),
        "execute PowerShell",
    )
}

fn run_command(command: &mut Command, description: &str) -> String {
    let output = command
        .output()
        .unwrap_or_else(|err| panic!("failed to {description}: {err}"));
    assert!(
        output.status.success(),
        "failed to {description} (status {}):\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{description} produced non-UTF-8 output: {err}"))
}
