pub fn words(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;

    for item in value.chars() {
        if item == '_' || item == '-' || item == ' ' || item == '.' || item == '/' {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            previous_lower = false;
            continue;
        }

        if item.is_ascii_uppercase() && previous_lower && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }

        previous_lower = item.is_ascii_lowercase() || item.is_ascii_digit();
        current.push(item);
    }

    if !current.is_empty() {
        out.push(current);
    }

    out.into_iter().map(|word| word.to_lowercase()).collect()
}

pub fn pascal(value: &str) -> String {
    words(value).into_iter().map(capitalise).collect()
}

pub fn camel(value: &str) -> String {
    let parts = words(value);
    let mut out = String::new();

    for (index, word) in parts.into_iter().enumerate() {
        if index == 0 {
            out.push_str(&word);
        } else {
            out.push_str(&capitalise(word));
        }
    }

    out
}

pub fn snake(value: &str) -> String {
    words(value).join("_")
}

pub fn kebab(value: &str) -> String {
    words(value).join("-")
}

pub fn screaming(value: &str) -> String {
    words(value).join("_").to_uppercase()
}

fn capitalise(word: String) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub fn npm_platform(triple: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match triple {
        "aarch64-apple-darwin" => Some(("darwin-arm64", "darwin", "arm64")),
        "x86_64-apple-darwin" => Some(("darwin-x64", "darwin", "x64")),
        "x86_64-unknown-linux-gnu" => Some(("linux-x64", "linux", "x64")),
        "aarch64-unknown-linux-gnu" => Some(("linux-arm64", "linux", "arm64")),
        "x86_64-unknown-linux-musl" => Some(("linux-x64-musl", "linux", "x64")),
        "x86_64-pc-windows-msvc" => Some(("win32-x64", "win32", "x64")),
        _ => None,
    }
}

pub fn wheel_platform(triple: &str) -> Option<&'static str> {
    match triple {
        "aarch64-apple-darwin" => Some("macosx_11_0_arm64"),
        "x86_64-apple-darwin" => Some("macosx_10_12_x86_64"),
        "x86_64-unknown-linux-gnu" => Some("manylinux_2_35_x86_64"),
        "aarch64-unknown-linux-gnu" => Some("manylinux_2_35_aarch64"),
        "x86_64-unknown-linux-musl" => Some("musllinux_1_2_x86_64"),
        "x86_64-pc-windows-msvc" => Some("win_amd64"),
        _ => None,
    }
}

pub fn go_platform(triple: &str) -> Option<(&'static str, &'static str)> {
    match triple {
        "aarch64-apple-darwin" => Some(("darwin", "arm64")),
        "x86_64-apple-darwin" => Some(("darwin", "amd64")),
        "x86_64-unknown-linux-gnu" => Some(("linux", "amd64")),
        "aarch64-unknown-linux-gnu" => Some(("linux", "arm64")),
        "x86_64-unknown-linux-musl" => Some(("linux", "amd64")),
        "x86_64-pc-windows-msvc" => Some(("windows", "amd64")),
        _ => None,
    }
}

pub fn binary_name(triple: &str) -> &'static str {
    if triple.contains("windows") { "wred.exe" } else { "wred" }
}
