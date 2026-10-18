//! "Audio follows": the Zone headset keeps a link to every computer (USB receiver on one,
//! Bluetooth on the others) and plays whichever one is sending sound. Handing the mouse
//! over makes this computer let go of the headset and the other one take it as its output,
//! so the sound moves together with the mouse and keyboard.

//!
//! The output the person picked is respected: letting go only happens while the headset is
//! the output, and it goes back to whatever was in use before the headset was taken.

use std::path::PathBuf;

fn state_file(name: &str) -> PathBuf {
    dirs::runtime_dir().unwrap_or_else(std::env::temp_dir).join("hidpilot").join(name)
}

/// The output in use before the headset was taken, to give it back on release.
fn previous() -> PathBuf {
    state_file("previous-output")
}

fn remember(name: &str) {
    let p = previous();
    let _ = p.parent().map(std::fs::create_dir_all);
    let _ = std::fs::write(p, name);
}

fn remembered() -> Option<String> {
    std::fs::read_to_string(previous()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(target_os = "linux")]
mod imp {
    use anyhow::{Context, Result, bail};
    use std::process::Command;

    struct Sink {
        id: String,
        name: String,
        default: bool,
    }

    /// Sinks from `wpctl status -n`: "│  *   55. alsa_output.usb-... [vol: 0.94]".
    fn sinks() -> Result<Vec<Sink>> {
        let out = Command::new("wpctl").args(["status", "-n"]).output().context("wpctl not found")?;
        Ok(parse(&String::from_utf8_lossy(&out.stdout)))
    }

    fn parse(text: &str) -> Vec<Sink> {
        let mut list = Vec::new();
        let mut inside = false;
        for line in text.lines() {
            if line.contains("Sinks:") {
                inside = true;
                continue;
            }
            if line.contains("endpoints:") || line.contains("Sources:") {
                inside = false;
            }
            if !inside {
                continue;
            }
            let l = line.trim_start_matches(|c: char| c.is_whitespace() || "│├└─*".contains(c));
            let default = line[..line.len() - l.len()].contains('*');
            if let Some((id, rest)) = l.split_once(". ") {
                if let (true, Some(name)) = (id.chars().all(|c| c.is_ascii_digit()), rest.split_whitespace().next()) {
                    list.push(Sink { id: id.into(), name: name.into(), default });
                }
            }
        }
        list
    }

    fn is_headset(name: &str, bt: Option<&str>) -> bool {
        name.contains("Logitech_Zone") || bt.is_some_and(|a| name.starts_with(&format!("bluez_output.{}", a.replace(':', "_"))))
    }

    fn set_default(s: &Sink) -> Result<()> {
        if !Command::new("wpctl").args(["set-default", &s.id]).status()?.success() {
            bail!("wpctl set-default {} failed", s.id);
        }
        Ok(())
    }

    pub fn claim() -> Result<String> {
        let bt = hidpilot::bluetooth::zone_headset().map(|b| b.address);
        let list = sinks()?;
        // The receiver sounds better and also carries the controls, so it wins over Bluetooth.
        let sink = list
            .iter()
            .find(|s| s.name.contains("Logitech_Zone"))
            .or_else(|| list.iter().find(|s| is_headset(&s.name, bt.as_deref())))
            .context("the headset is not an output here (is it on and linked to this computer?)")?;
        if let Some(cur) = list.iter().find(|s| s.default && !is_headset(&s.name, bt.as_deref())) {
            super::remember(&cur.name);
        }
        set_default(sink)?;
        Ok(sink.name.clone())
    }

    pub fn release() -> Result<Option<String>> {
        let bt = hidpilot::bluetooth::zone_headset().map(|b| b.address);
        let list = sinks()?;
        let Some(sink) = fallback(&list, bt.as_deref(), super::remembered().as_deref()) else { return Ok(None) };
        set_default(sink)?;
        Ok(Some(sink.name.clone()))
    }

    /// Where the sound goes when the headset leaves: nowhere new unless the headset is the
    /// output; then the output used before it, or else the monitor or any other one.
    fn fallback<'a>(list: &'a [Sink], bt: Option<&str>, before: Option<&str>) -> Option<&'a Sink> {
        if !list.iter().any(|s| s.default && is_headset(&s.name, bt)) {
            return None;
        }
        let others: Vec<&Sink> = list.iter().filter(|s| !is_headset(&s.name, bt) && !s.name.contains("easyeffects")).collect();
        others
            .iter()
            .find(|s| Some(s.name.as_str()) == before)
            .or_else(|| others.iter().find(|s| s.name.contains("hdmi")))
            .or_else(|| others.first())
            .copied()
    }

    /// Which headset output exists right now, if any: receiver first, then Bluetooth.
    fn headset_sink(list: &[Sink], bt: Option<&str>) -> Option<String> {
        list.iter()
            .find(|s| s.name.contains("Logitech_Zone"))
            .or_else(|| list.iter().find(|s| is_headset(&s.name, bt)))
            .map(|s| s.name.clone())
    }

    /// Follows the headset as it comes and goes: when it appears it becomes the output, when
    /// it goes away the output used before comes back, as if it had never been there. Only
    /// those moments change anything, so an output picked by hand in between is kept.
    pub fn watch(enabled: impl Fn() -> bool) {
        let mut last: Option<Option<String>> = None;
        // Whether the headset was the output on the previous look: only then does its
        // leaving move the sound (otherwise the person was already using something else).
        let mut was_output = false;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if !enabled() {
                last = None;
                continue;
            }
            let bt = hidpilot::bluetooth::zone_headset();
            let Ok(list) = sinks() else { continue };
            let now = headset_sink(&list, bt.as_ref().map(|b| b.address.as_str()));
            // Linked to both the receiver and Bluetooth of this same computer: keep the receiver.
            if let (Some(n), Some(b)) = (&now, &bt) {
                if n.contains("Logitech_Zone") && b.connected {
                    let _ = Command::new("bluetoothctl").args(["disconnect", &b.address]).output();
                }
            }
            let had = std::mem::replace(&mut was_output, list.iter().any(|s| s.default && Some(&s.name) == now.as_ref()));
            let Some(before) = last.replace(now.clone()) else { continue }; // first look: change nothing
            if before == now {
                continue;
            }
            let bt = bt.map(|b| b.address);
            match &now {
                Some(name) => {
                    if let Some(cur) = list.iter().find(|s| s.default && !is_headset(&s.name, bt.as_deref())) {
                        super::remember(&cur.name);
                    }
                    if let Some(sink) = list.iter().find(|s| &s.name == name) {
                        let _ = set_default(sink);
                        println!("flow: headset is here, sound → {name}");
                    }
                }
                None if had => {
                    let others: Vec<&Sink> = list.iter().filter(|s| !s.name.contains("easyeffects")).collect();
                    let before = super::remembered();
                    if let Some(sink) = others.iter().find(|s| Some(s.name.as_str()) == before.as_deref()) {
                        let _ = set_default(sink);
                        println!("flow: headset gone, sound back to {}", sink.name);
                    }
                }
                None => {}
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const STATUS: &str = "Audio\n ├─ Sinks:\n │      52. alsa_output.pci-0000_09_00.4.iec958-stereo [vol: 0.40]\n │  *   55. alsa_output.usb-Logitech_Zone_Vibe_Wireless_123-00.analog-stereo [vol: 0.94]\n │      70. alsa_output.pci-0000_07_00.1.hdmi-stereo [vol: 0.40]\n │      71. bluez_output.44_73_D6_C9_5B_36.1 [vol: 0.50]\n │      80. easyeffects_sink                    [vol: 1.00]\n │  \n ├─ Sink endpoints:\n ├─ Sources:\n │      90. alsa_input.usb-Logitech_Zone_Vibe_Wireless_123-00.mono [vol: 1.00]\n";

        #[test]
        fn reads_sinks_and_picks_outputs() {
            let list = parse(STATUS);
            assert_eq!(list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["52", "55", "70", "71", "80"]);
            let bt = Some("44:73:D6:C9:5B:36");
            assert!(is_headset(&list[1].name, bt) && is_headset(&list[3].name, bt) && !is_headset(&list[2].name, bt));
            assert_eq!(fallback(&list, bt, None).unwrap().id, "70");
            assert_eq!(fallback(&list, bt, Some("alsa_output.pci-0000_09_00.4.iec958-stereo")).unwrap().id, "52");
            // Something else picked by hand: leave it alone.
            let other = parse(&STATUS.replace("│  *   55.", "│      55.").replace("│      52.", "│  *   52."));
            assert!(fallback(&other, bt, None).is_none());
        }
    }
}

/// macOS has no command line tool for the default output; `SwitchAudioSource`
/// (`brew install switchaudio-osx`) is the usual one.
#[cfg(target_os = "macos")]
mod imp {
    use anyhow::{Context, Result, bail};
    use std::process::Command;

    fn outputs() -> Result<Vec<String>> {
        let out = Command::new("SwitchAudioSource")
            .args(["-a", "-t", "output"])
            .output()
            .context("SwitchAudioSource not found: brew install switchaudio-osx")?;
        Ok(String::from_utf8_lossy(&out.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
    }

    fn select(name: &str) -> Result<String> {
        if !Command::new("SwitchAudioSource").args(["-t", "output", "-s", name]).status()?.success() {
            bail!("SwitchAudioSource could not select {name}");
        }
        Ok(name.into())
    }

    fn current() -> Result<String> {
        let out = Command::new("SwitchAudioSource").args(["-c", "-t", "output"]).output()?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub fn claim() -> Result<String> {
        let name = outputs()?.into_iter().find(|n| n.contains("Zone")).context("the headset is not an output here (is it connected over Bluetooth?)")?;
        let cur = current()?;
        if !cur.contains("Zone") && !cur.is_empty() {
            super::remember(&cur);
        }
        select(&name)
    }

    pub fn release() -> Result<Option<String>> {
        if !current()?.contains("Zone") {
            return Ok(None);
        }
        let all = outputs()?;
        let others: Vec<&String> = all.iter().filter(|n| !n.contains("Zone")).collect();
        let before = super::remembered();
        let name = others
            .iter()
            .find(|n| Some(n.as_str()) == before.as_deref())
            .or_else(|| others.iter().find(|n| n.contains("MacBook") || n.contains("Speakers") || n.contains("Alto-falantes")))
            .or_else(|| others.first())
            .context("no other output")?;
        select(name).map(Some)
    }
}

/// Windows: the AudioDeviceCmdlets PowerShell module (`Install-Module AudioDeviceCmdlets`),
/// since Windows has no built-in command for the default output.
#[cfg(windows)]
mod imp {
    use anyhow::{Context, Result, bail};
    use std::process::Command;

    struct Output {
        id: String,
        name: String,
        default: bool,
    }

    fn powershell(script: &str) -> Result<String> {
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .context("powershell not found")?;
        if !out.status.success() {
            bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn outputs() -> Result<Vec<Output>> {
        let text = powershell(
            "Import-Module AudioDeviceCmdlets -ErrorAction Stop; Get-AudioDevice -List | Where-Object Type -eq 'Playback' | ForEach-Object { \"$($_.ID)`t$($_.Default)`t$($_.Name)\" }",
        )
        .context("AudioDeviceCmdlets missing: Install-Module AudioDeviceCmdlets -Scope CurrentUser")?;
        Ok(text
            .lines()
            .filter_map(|l| {
                let mut f = l.trim().splitn(3, '\t');
                Some(Output { id: f.next()?.into(), default: f.next()? == "True", name: f.next()?.into() })
            })
            .collect())
    }

    fn select(o: &Output) -> Result<String> {
        powershell(&format!("Import-Module AudioDeviceCmdlets; Set-AudioDevice -ID '{}' | Out-Null", o.id.replace('\'', "")))?;
        Ok(o.name.clone())
    }

    pub fn claim() -> Result<String> {
        let all = outputs()?;
        let zone = all.iter().find(|o| o.name.contains("Zone")).context("the headset is not an output here (is it connected over Bluetooth?)")?;
        if let Some(cur) = all.iter().find(|o| o.default && !o.name.contains("Zone")) {
            super::remember(&cur.name);
        }
        select(zone)
    }

    pub fn release() -> Result<Option<String>> {
        let all = outputs()?;
        if !all.iter().any(|o| o.default && o.name.contains("Zone")) {
            return Ok(None);
        }
        let before = super::remembered();
        let others: Vec<&Output> = all.iter().filter(|o| !o.name.contains("Zone")).collect();
        let o = others.iter().find(|o| Some(o.name.as_str()) == before.as_deref()).or_else(|| others.first()).context("no other output")?;
        select(o).map(Some)
    }
}

/// Keeps the output following the headset as it comes and goes (Linux).
pub fn watch(enabled: impl Fn() -> bool + Send + 'static) {
    #[cfg(target_os = "linux")]
    std::thread::spawn(move || imp::watch(enabled));
    #[cfg(not(target_os = "linux"))]
    let _ = enabled;
}

/// The mouse arrived here: make the headset this computer's output.
pub fn claim() {
    match imp::claim() {
        Ok(out) => println!("flow: sound → {out}"),
        Err(e) => eprintln!("flow: sound stays: {e:#}"),
    }
}

/// The mouse left: stop sending sound to the headset so the other computer gets it.
pub fn release() {
    match imp::release() {
        Ok(Some(out)) => println!("flow: sound here → {out}"),
        Ok(None) => println!("flow: the headset was not the output here; left as it is"),
        Err(e) => eprintln!("flow: sound stays: {e:#}"),
    }
}
