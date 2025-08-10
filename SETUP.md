# Setup Instructions for Automated Publishing

This document provides instructions for repository maintainers to complete the setup for automated publishing to crates.io.

## Required Setup

### 1. Configure Crates.io API Token

To enable automated publishing to crates.io, you need to create and configure a crates.io API token:

#### Steps:

1. **Get a crates.io account**:
   - Sign up or log in at https://crates.io/

2. **Generate an API token**:
   - Go to https://crates.io/me
   - Click on "API Tokens" 
   - Click "New Token"
   - Name: `rust-guardian-ci` (or any descriptive name)
   - Scope: Select `publish-update` to allow publishing and updating existing crates
   - Click "Create"
   - **Important**: Copy the token immediately - you won't be able to see it again

3. **Add the token to GitHub repository secrets**:
   - Go to the GitHub repository: https://github.com/cloudfunnels/rust-guardian
   - Navigate to: Settings → Secrets and variables → Actions
   - Click "New repository secret"
   - Name: `CRATES_TOKEN`
   - Value: Paste the token you copied from crates.io
   - Click "Add secret"

### 2. Verify Setup

Once the token is configured, you can test the automation:

1. **Test CI workflow**:
   ```bash
   # Any push to main will trigger CI
   git push origin main
   ```

2. **Test release workflow**:
   ```bash
   # Update version in Cargo.toml first
   git tag v0.1.0
   git push origin v0.1.0
   ```

## What Happens After Setup

### On Every Push/PR:
- ✅ Code formatting checks
- ✅ Linting with clippy  
- ✅ Tests on stable, beta, nightly Rust
- ✅ Security audit
- ✅ Documentation validation

### On Version Tag (e.g., `v0.1.0`):
- ✅ All CI checks
- ✅ Version verification
- ✅ Automatic crates.io publishing
- ✅ GitHub release creation
- ✅ Binary artifact generation

## Repository URLs After Publishing

- **Crates.io**: https://crates.io/crates/rust-guardian
- **Documentation**: https://docs.rs/rust-guardian (automatically updated)
- **Repository**: https://github.com/cloudfunnels/rust-guardian

## Troubleshooting

### Common Issues:

1. **"Invalid token" error**:
   - Verify the CRATES_TOKEN secret is correctly set
   - Ensure the token has `publish-update` scope

2. **"Version already exists" error**:
   - Crates.io doesn't allow republishing the same version
   - Increment the version in Cargo.toml and create a new tag

3. **Test failures**:
   - The release will be blocked if any tests fail
   - Fix issues and create a new tag

### Getting Help:

- Check GitHub Actions logs for detailed error messages
- See `RELEASE.md` for detailed release process documentation
- Create an issue if you encounter problems

## Security Notes

- The CRATES_TOKEN should only have `publish-update` scope (not admin access)
- Only repository maintainers should have access to manage secrets
- The token is only used for publishing, not for downloading or other operations
- Consider rotating the token periodically for security

## Ready to Publish!

Once the CRATES_TOKEN secret is configured, the repository is ready for automated publishing to crates.io. The first release can be created by:

1. Ensuring the version in `Cargo.toml` is correct (e.g., `0.1.0`)
2. Creating and pushing a version tag: `git tag v0.1.0 && git push origin v0.1.0`
3. The automation will handle the rest!

The crate will then be available at https://crates.io/crates/rust-guardian and documentation will be published at https://docs.rs/rust-guardian.