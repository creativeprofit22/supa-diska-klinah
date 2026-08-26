use tauri::{Runtime, Url, plugin::TauriPlugin};

fn has_no_credentials(url: &Url) -> bool {
    url.username().is_empty() && url.password().is_none()
}

pub(crate) fn is_allowed_navigation(url: &Url, development: bool) -> bool {
    has_no_credentials(url)
        && ((url.scheme() == "http"
            && url.host_str() == Some("tauri.localhost")
            && url.port().is_none())
            || (development
                && url.scheme() == "http"
                && url.host_str() == Some("127.0.0.1")
                && url.port() == Some(1420)))
}

pub(crate) fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("local-navigation")
        .on_navigation(|webview, url| {
            webview.label() == "main" && is_allowed_navigation(url, cfg!(dev))
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::is_allowed_navigation;

    #[test]
    fn production_allows_only_the_packaged_origin() {
        assert!(is_allowed_navigation(
            &"http://tauri.localhost/settings".parse().unwrap(),
            false,
        ));
        for denied in [
            "https://tauri.localhost/",
            "http://tauri.localhost.evil.invalid/",
            "http://user@tauri.localhost/",
            "http://127.0.0.1:1420/",
            "https://example.com/",
            "file:///C:/Windows/System32/cmd.exe",
        ] {
            assert!(!is_allowed_navigation(&denied.parse().unwrap(), false));
        }
    }

    #[test]
    fn development_allows_only_the_exact_vite_origin() {
        assert!(is_allowed_navigation(
            &"http://127.0.0.1:1420/dashboard".parse().unwrap(),
            true,
        ));
        for denied in [
            "http://localhost:1420/",
            "http://127.0.0.1/",
            "http://127.0.0.1:1421/",
            "https://127.0.0.1:1420/",
            "http://127.0.0.1.evil.invalid:1420/",
        ] {
            assert!(!is_allowed_navigation(&denied.parse().unwrap(), true));
        }
    }
}
