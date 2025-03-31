use std::process::Command;

pub fn get_user() -> Option<String> {
    let Ok(o) = Command::new("/usr/bin/stat")
        .args(["-f", "\"%Su\"", "/dev/console"])
        .output()
    else {
        eprintln!("error getting currently logged in user");
        return None;
    };
    if !o.status.success() {
        return None;
    }
    let Ok(o) = String::from_utf8(o.stdout) else {
        eprintln!("error converting stdout to string");
        return None;
    };
    return Some(o.replace(|c| !char::is_alphabetic(c), ""));
}
