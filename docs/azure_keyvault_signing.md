# Azure Key Vault Code Signing Setup

This document describes how to set up Azure Key Vault code signing for the FSCT Driver Service Windows installer in GitHub Actions.

## Overview

The project supports two signing methods:
1. **Local Certificate Signing** - Uses a certificate from the local certificate store (for local builds)
2. **Azure Key Vault Signing** - Uses certificates stored in Azure Key Vault (for CI/CD builds)

## Azure Key Vault Prerequisites

Before setting up GitHub Actions, you need:

1. **Azure Key Vault** with a code signing certificate
2. **Azure App Registration** with permissions to access the Key Vault
3. **Certificate** uploaded to the Key Vault for code signing

### Azure Setup Steps

1. **Create Azure Key Vault**:
   - Create a Key Vault in your Azure subscription
   - Note the Key Vault URL (e.g., `https://your-vault.vault.azure.net/`)

2. **Upload Code Signing Certificate**:
   - Upload your code signing certificate to the Key Vault
   - Note the certificate name

3. **Create App Registration**:
   - Create an Azure App Registration (Service Principal)
   - Generate a client secret
   - Note the Application (Client) ID and Tenant ID

4. **Grant Key Vault Permissions**:
   - In Key Vault Access Policies, grant the App Registration:
     - Certificate permissions: Get, List
     - Key permissions: Get, List, Sign
     - Secret permissions: Get, List

## GitHub Secrets Configuration

Configure the following secrets in your GitHub repository settings (Settings → Secrets and variables → Actions):

### Required Secrets

| Secret Name | Description | Example |
|-------------|-------------|---------|
| `AZURE_KEY_VAULT_URL` | Azure Key Vault URL | `https://your-vault.vault.azure.net/` |
| `AZURE_CERTIFICATE_NAME` | Certificate name in Key Vault | `CodeSigningCert` |
| `AZURE_TENANT_ID` | Azure tenant ID | `12345678-1234-1234-1234-123456789012` |
| `AZURE_CLIENT_ID` | Azure App Registration client ID | `87654321-4321-4321-4321-210987654321` |
| `AZURE_CLIENT_SECRET` | Azure App Registration client secret | `your-client-secret-value` |

### Setting Up Secrets

1. Go to your GitHub repository
2. Navigate to **Settings** → **Secrets and variables** → **Actions**
3. Click **New repository secret**
4. Add each secret with the exact name and corresponding value

## How It Works

### GitHub Actions Workflow

The workflow automatically detects if Azure Key Vault signing is configured:

- **If all Azure Key Vault secrets are present**: Builds and signs the installer using Azure Key Vault
- **If any Azure Key Vault secret is missing**: Builds an unsigned installer

### Build Process

1. **Tool Installation**: Installs AzureSignTool via dotnet global tool
2. **Conditional Build**: 
   - Signed build: Calls `build_windows_installer.ps1` with Azure Key Vault parameters
   - Unsigned build: Calls `build_windows_installer.ps1` with `-NoSign` flag
3. **Artifact Upload**: Uploads the resulting installer (signed or unsigned)

### PowerShell Script Integration

The `build_windows_installer.ps1` script supports Azure Key Vault signing with these parameters:

```powershell
.\build_windows_installer.ps1 -UseAzureKeyVault `
  -AzureKeyVaultUrl "https://your-vault.vault.azure.net/" `
  -AzureCertificateName "CodeSigningCert" `
  -AzureTenantId "12345678-1234-1234-1234-123456789012" `
  -AzureClientId "87654321-4321-4321-4321-210987654321" `
  -AzureClientSecret "your-client-secret-value"
```

## Security Considerations

1. **Client Secret**: Store as GitHub secret, never commit to repository
2. **Least Privilege**: Grant minimal required permissions to the App Registration
3. **Key Vault Access**: Restrict Key Vault access to necessary services only
4. **Certificate Management**: Regularly rotate certificates and update Key Vault

## Troubleshooting

### Common Issues

1. **"Failed to sign with Azure Key Vault"**:
   - Verify all Azure Key Vault secrets are correctly set
   - Check App Registration permissions on Key Vault
   - Ensure certificate exists in Key Vault

2. **"AzureSignTool not found"**:
   - Verify AzureSignTool installation step in GitHub Actions
   - Check if dotnet global tools path is in PATH

3. **Authentication failures**:
   - Verify Tenant ID, Client ID, and Client Secret
   - Check App Registration is not expired
   - Ensure Key Vault access policies are correctly configured

### Verification

To verify the setup:

1. Check GitHub Actions logs for signing messages
2. Verify the installer is signed using `signtool verify /pa /v installer.exe`
3. Check certificate details match your Azure Key Vault certificate

## Local Development

For local development, you can still use the Azure Key Vault signing by providing the parameters directly:

```powershell
.\script\build_windows_installer.ps1 -UseAzureKeyVault `
  -AzureKeyVaultUrl "https://your-vault.vault.azure.net/" `
  -AzureCertificateName "CodeSigningCert" `
  -AzureTenantId "your-tenant-id" `
  -AzureClientId "your-client-id" `
  -AzureClientSecret "your-client-secret"
```

Or use local certificate signing (default behavior without `-UseAzureKeyVault`).