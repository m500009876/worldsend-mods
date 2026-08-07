use std::process::Command;

/// Returns the command name/path to invoke Java, if one is found on the
/// system (checks JAVA_HOME then PATH). Does not verify the version.
pub fn find_java() -> Option<String> {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let candidate = if cfg!(target_os = "windows") {
            format!("{}\\bin\\javaw.exe", home)
        } else {
            format!("{}/bin/java", home)
        };
        if std::path::Path::new(&candidate).exists() {
            return Some(candidate);
        }
    }

    let exe = if cfg!(target_os = "windows") {
        "javaw"
    } else {
        "java"
    };

    let check = if cfg!(target_os = "windows") {
        Command::new("where").arg(exe).output()
    } else {
        Command::new("which").arg(exe).output()
    };

    match check {
        Ok(out) if out.status.success() => Some(exe.to_string()),
        _ => None,
    }
}

/// Command used specifically for running installer jars where we want to
/// see console output (java, not javaw), even on Windows.
pub fn find_java_console() -> Option<String> {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let candidate = if cfg!(target_os = "windows") {
            format!("{}\\bin\\java.exe", home)
        } else {
            format!("{}/bin/java", home)
        };
        if std::path::Path::new(&candidate).exists() {
            return Some(candidate);
        }
    }

    let check = if cfg!(target_os = "windows") {
        Command::new("where").arg("java").output()
    } else {
        Command::new("which").arg("java").output()
    };

    match check {
        Ok(out) if out.status.success() => Some("java".to_string()),
        _ => None,
    }
}
