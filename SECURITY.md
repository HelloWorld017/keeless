# Security Policy
## Supported Versions

Security updates are provided **only for the latest stable version**. If you discover a vulnerability, please verify that it exists in the latest version before submitting a report.

| Version | Supported |
| :--- | :--- |
| Latest | :white_check_mark: |
| < Latest | :x: |


## Reporting a Vulnerability
Thank you for helping keep this project secure. Please **do not open public issues** for security vulnerabilities. Instead, follow the secure reporting procedures outlined below.

### General Vulnerability Reporting
For non-sensitive security issues, please use the GitHub **Private Vulnerability Reporting** feature.

### Encrypted Sensitive Reporting
If you need to share sensitive details, exploit code, or private data, please use the following command to encrypt your files using `age` before sending them to the security maintainers:

```bash
# 1. Compress your sensitive files into a zip archive
zip -r detail.zip README.md proof-of-concept

# 2. Encrypt the file using the project's public key
curl -s https://github.com/HelloWorld017.keys | age -a -R - -o detail.zip.enc detail.zip

# 3. Send the resulting 'detail.zip.enc' file to the security contact
```

## PoC Guidelines
To help our team verify vulnerabilities quickly and accurately, please include a valid, working PoC (Proof of Concept) with your report.

### Requirements
* **Non-Destructive**: The PoC must not cause permanent damage, data loss, or service disruption (DoS) to the target system.
* **Reproducible**: Provide clear instructions, including the example code and execution environment (e.g. flake.nix, flake.lock) and step-by-step reproduction steps.
* **Minimal**: Include only the core logic necessary to demonstrate the vulnerability.
