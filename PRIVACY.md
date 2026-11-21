# VTechDev Privacy Statement for Netray

**Last Updated:** November 21, 2026  
**Effective Date:** November 21, 2026  
**Publisher:** VTechDev  
**Product:** Netray (Windows & Linux)  

---

## Overview

Your privacy is important to VTechDev ("we", "us", or "our"). This Privacy Statement explains what personal data Netray processes, how we process it, and for what purposes. 

Netray is designed with a fundamental **privacy-by-architecture** philosophy. Netray is a client-side network tunneling and proxy utility. **We do not operate central proxy servers, we do not track your browsing activity, we do not collect telemetry or analytics, and we do not store your data on external servers.** All configurations, keys, and usage statistics remain exclusively on your local device.

---

## 1. Personal Data We Collect

### A. Data Stored Locally on Your Device
When you use Netray, the following data is created and saved solely on your local computer (`%APPDATA%\Netray\` on Windows, or `~/.config/netray/` on Linux):
- **Server and Profile Configurations:** Server hostnames, IP addresses, port numbers, network protocols (V2Ray, VLess, VMess, Trojan, Shadowsocks, SSH), cryptographic public keys, and user identifiers (e.g., UUIDs or passwords) that you enter or import.
- **Routing Rules & Application Whitelists:** Local rules and process names (e.g., `chrome.exe`) that you configure to bypass or route through the tunnel.
- **Local Usage & Traffic Counters:** Aggregated daily byte counts (upload/download totals) recorded locally in `usage.json` for your personal review.
- **Diagnostic Console Logs:** Real-time operational messages (e.g., connection status, handshake events) generated during an active session and displayed in the application cockpit.

*None of this data is ever transmitted to VTechDev or any third-party infrastructure.*

### B. Network Communications Made by Netray
Netray initiates network connections under only two circumstances:
1. **Routing Traffic to Your Chosen Endpoint:** When you activate a tunnel, Netray forwards your network traffic directly to the third-party proxy server, VPN server, or SSH host specified in your active profile. VTechDev has no access to, ownership of, or visibility into these servers or the traffic routed through them.
2. **Version & Update Checks:** Netray periodically checks the public GitHub Releases API (`api.github.com/repos/venujatech-byte/Netray/releases/latest`) to determine if a newer version of the software is available. When this check occurs, standard network metadata (such as your public IP address and user-agent) is transmitted to GitHub in accordance with [GitHub's Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-privacy-statement).

### C. Data We Do NOT Collect
We do **not**:
- Collect, log, or monitor your DNS queries, browsing history, or visited URLs.
- Collect hardware identifiers, advertising IDs, or unique device fingerprints.
- Use analytics frameworks, tracking pixels, crash telemetry SDKs, or third-party tracking libraries.
- Require or process personal identifiers such as names, physical addresses, phone numbers, or payment card details.

---

## 2. How We Use Personal Data

VTechDev processes the data you provide strictly to deliver the core functionality of Netray on your local machine:
- **Establishing Tunnels:** To configure your local network adapter (WinTun driver) and system proxy settings so traffic routes to your specified servers.
- **Displaying Metrics:** To calculate and display real-time connection latency and daily transfer volume in the user interface cockpit.
- **Application Preferences:** To remember your theme preferences (dark/light mode), window layout, and auto-reconnect preferences across sessions.
- **Software Updates:** To notify you when security patches or software updates are published.

---

## 3. Reasons We Share Personal Data

**We do not sell, rent, monetize, or disclose your personal data to any third party.** 

Because VTechDev does not operate remote servers for Netray, we do not store, view, or retain your data. Your data only leaves your machine when transmitted directly to the remote server endpoints (e.g., your chosen V2Ray, VLess, or SSH host) that you deliberately configure.

---

## 4. How to Access and Control Your Personal Data

You maintain complete ownership and control over all data associated with Netray:
- **View or Modify Configurations:** You can inspect, edit, export, or delete any profile directly within the Netray user interface at any time.
- **Reset Configuration and Usage Data:** You can delete all saved profiles and metrics by deleting the local data directory:
  - **Windows:** `%APPDATA%\Netray\`
  - **Linux:** `~/.config/netray/`
- **Uninstall Netray:** Uninstalling Netray removes all application binaries and temporary files from your system.
- **Revoke System Network Permissions:** Disconnecting the tunnel or closing the app automatically tears down the WinTun virtual adapter and restores your default system internet proxy settings.

---

## 5. Security of Personal Data

We are committed to protecting the security of your data:
- **Local Isolation:** Profile configurations and usage logs are stored locally in user-scoped application directories protected by your operating system's file access controls.
- **Memory Protection:** Cryptographic keys and proxy credentials in memory are managed with bounded scope and discarded upon process termination.
- **Modern Encryption:** Netray exclusively implements standardized, robust cryptographic protocols (TLS 1.3, SSH-2, ChaCha20-Poly1305, AES-GCM) for tunnel security.
- **Source Code Verification:** Netray is open-source software, allowing the community to inspect, verify, and validate all application behaviors and network interactions.

---

## 6. Where We Store and Process Data

All personal data and configuration files processed by Netray are stored and processed **locally on your device**. VTechDev does not transfer or process your data across state, national, or international borders.

---

## 7. Data Retention

Because all data resides exclusively on your local device, data retention is managed directly by you. Netray keeps your saved profiles and daily traffic totals on your disk until you modify or delete them, or until you uninstall the software.

---

## 8. Children's Privacy

Netray does not knowingly collect or solicit any personal information from children under the age of 13 (or under 16 in the European Union). Because Netray does not collect personal data from any user, no children's data is processed or retained.

---

## 9. Changes to This Privacy Statement

We may update this Privacy Statement from time to time to reflect changes in our software, legal requirements, or Microsoft Store policies. When we post changes, we will update the "Last Updated" date at the top of this statement. We encourage you to periodically review this statement to stay informed about how Netray protects your privacy.

---

## 10. How to Contact Us

If you have any questions, concerns, or requests regarding this Privacy Statement or Netray's privacy practices, you can contact us:

- **Publisher:** VTechDev
- **Email:** [venujatech@gmail.com](mailto:venujatech@gmail.com)
- **GitHub Repository & Issue Tracker:** [https://github.com/venujatech-byte/Netray/issues](https://github.com/venujatech-byte/Netray/issues)
