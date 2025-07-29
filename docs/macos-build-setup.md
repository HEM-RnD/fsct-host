# macOS Build Setup for GitHub Actions

This document describes how to set up the macOS build process in GitHub Actions, including the necessary configuration in the Apple Developer Portal and GitHub repository secrets.

## Apple Developer Portal Configuration

### 1. Apple Developer Program Membership

Ensure you have an active Apple Developer Program membership. This is required for code signing and notarization.

### 2. Create Certificates

You need two types of certificates:

1. **Developer ID Application Certificate**: Used to sign the application binary
2. **Developer ID Installer Certificate**: Used to sign the installer package

To create these certificates:

1. Go to [Apple Developer Portal](https://developer.apple.com/account/resources/certificates/list)
2. Click the "+" button to create a new certificate
3. Select "Developer ID Application" or "Developer ID Installer" as the certificate type
4. Follow the instructions to create a Certificate Signing Request (CSR) using Keychain Access
5. Upload the CSR and download the certificate
6. Double-click the downloaded certificate to add it to your keychain

### 3. Create App-Specific Password

For notarization, you need an app-specific password:`

1. Go to [Apple ID Account Page](https://appleid.apple.com/)
2. Sign in with your Apple ID
3. In the "Security" section, click "Generate Password..." under "App-Specific Passwords"
4. Give the password a name (e.g., "GitHub Actions Notarization")
5. Save the generated password securely

### 4. Find Your Team ID

You'll need your Team ID for notarization:

1. Go to [Apple Developer Portal](https://developer.apple.com/account)
2. Your Team ID is displayed in the membership details or in the top-right corner

## GitHub Repository Configuration

### Required Secrets

Add the following secrets to your GitHub repository:

1. **`APPLE_ID`**: Your Apple ID email address
2. **`APPLE_TEAM_ID`**: Your Apple Developer Team ID
3. **`APPLE_APP_PASSWORD`**: The app-specific password you created
4. **`APPLE_CERTIFICATE_BASE64`**: Your Developer ID certificates (both Application and Installer) exported as a single P12 file and encoded in base64
5. **`APPLE_CERTIFICATE_PASSWORD`**: The password for your P12 certificate file

> **Note**: The certificate identifiers (`APPLE_DEVELOPER_ID_APP` and `APPLE_DEVELOPER_ID_INSTALLER`) and the keychain profile name (`APPLE_NOTARY_PROFILE`) are hardcoded in the `macos_service_package_builder.sh` script and don't need to be provided as secrets or passed as parameters. If you need to use different values, you can modify the script directly.

### Exporting Certificates as P12

To export both certificates as a single P12 file and encode it in base64:

1. Open Keychain Access
2. Find both your Developer ID Application and Developer ID Installer certificates
3. Select both certificates and their private keys (use Command key to select multiple items)
4. Right-click and select "Export Items..."
5. Save as a P12 file (e.g., certificates.p12) with a strong password
6. In Terminal, encode the P12 file to base64:
   ```
   base64 -i certificates.p12 | pbcopy
   ```
7. Paste the copied base64 string as the value for `APPLE_CERTIFICATE_BASE64` in GitHub secrets
8. Store the password you used as `APPLE_CERTIFICATE_PASSWORD` in GitHub secrets

> **Note**: Make sure both certificates and their private keys are included in the exported P12 file. You can verify this by importing the P12 file into a new keychain and checking that both certificates are present.

## GitHub Actions Workflow

The GitHub Actions workflow is configured to:

1. Set up a macOS runner
2. Install Rust with x86_64 and aarch64 targets
3. Install required tools (pandoc)
4. Create a temporary keychain and import your certificates
5. Set up a notarization profile using your Apple credentials
6. Build the macOS package using the `macos_service_package_builder.sh` script
7. Upload the package as an artifact and to GitHub releases (for release events)

### Signing and Notarization Process

The workflow handles signing and notarization as follows:

1. **Code Signing**: 
   - Always performed when certificates are available, regardless of branch
   - Skipped only if the certificate secrets are not configured in the repository

2. **Notarization**:
   - Only performed for specific branches and tags:
     - `develop` branch
     - `main` branch
     - `release/*` branches
     - `hotfix/*` branches
     - Version tags (`v*`)
   - Skipped for all other branches, even when certificates are available
   - Requires all notarization-related secrets to be configured

## Troubleshooting

### Certificate Issues

If you encounter certificate-related issues:

1. Ensure your certificates are valid and not expired
2. Check that the certificate names in the GitHub secrets match exactly with the certificate names in your keychain
3. Verify that the P12 file was exported correctly with both the certificate and private key

### Notarization Issues

If notarization fails:

1. Check the GitHub Actions logs for specific error messages
2. Verify that your Apple ID, Team ID, and app-specific password are correct
3. Ensure your Apple Developer Program membership is active
4. Check that the binary and package are properly signed before notarization

### Build Issues

If the build fails:

1. Check if all required tools are installed on the runner
2. Verify that the script has execution permissions (`chmod +x`)
3. Check for any errors in the build logs

## References

- [Apple Developer Documentation on Code Signing](https://developer.apple.com/documentation/security/code_signing)
- [Apple Developer Documentation on Notarization](https://developer.apple.com/documentation/security/notarizing_macos_software_before_distribution)
- [GitHub Actions Documentation](https://docs.github.com/en/actions)