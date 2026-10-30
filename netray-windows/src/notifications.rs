//! Desktop Notification Dispatcher for Netray Windows
//!
//! Provides platform-native desktop notification capabilities without heavy runtime dependencies.
//! On Linux: utilizes FreeDesktop-compliant `notify-send` with application identity "Netray".
//! On Windows: utilizes PowerShell WinRT Toast notification.

use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationUrgency {
    Low,
    Normal,
    Critical,
}

pub fn send_desktop_notification(title: &str, message: &str, urgency: NotificationUrgency) {
    #[cfg(target_os = "linux")]
    {
        send_linux_notification(title, message, urgency);
    }
    #[cfg(target_os = "windows")]
    {
        send_windows_notification(title, message, urgency);
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = (title, message, urgency);
    }
}

#[cfg(target_os = "linux")]
fn send_linux_notification(title: &str, message: &str, urgency: NotificationUrgency) {
    let urgency_str = match urgency {
        NotificationUrgency::Low => "low",
        NotificationUrgency::Normal => "normal",
        NotificationUrgency::Critical => "critical",
    };

    let _ = Command::new("notify-send")
        .args([
            "-a", "Netray",
            "-u", urgency_str,
            "-i", "network-vpn",
            title,
            message,
        ])
        .spawn();
}

#[cfg(target_os = "windows")]
fn send_windows_notification(title: &str, message: &str, _urgency: NotificationUrgency) {
    let clean_title = title.replace('"', "'");
    let clean_msg = message.replace('"', "'");
    let script = format!(
        r#"[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; $template = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); $textNodes = $template.GetElementsByTagName('text'); $textNodes.Item(0).AppendChild($template.CreateTextNode("{}")) > $null; $textNodes.Item(1).AppendChild($template.CreateTextNode("{}")) > $null; $notifier = [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Netray'); $toast = [Windows.UI.Notifications.ToastNotification]::new($template); $notifier.Show($toast);"#,
        clean_title, clean_msg
    );

    let _ = Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_functions_callable() {
        send_desktop_notification("Test Netray", "Notification test payload", NotificationUrgency::Normal);
    }
}

