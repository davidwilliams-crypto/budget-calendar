# Publishing a signed Windows release

`.github/workflows/release.yml` turns a version tag into a signed installer on the repository's Releases page. People download it there without a GitHub account. You do the Azure and GitHub setup below once; after that, each release is a tag push and a review.

## What a release run does

On a `vX.Y.Z` tag, a GitHub-hosted Windows machine:

1. checks that the tag matches the `budget-gui/Cargo.toml` version;
2. runs `scripts/release-gate.ps1`, where any failure or skipped step stops the release;
3. builds `budget-gui.exe` and confirms that `--version` and `--version-json` report that version;
4. confirms that the executable doesn't need the separately installed Visual C++ runtime;
5. signs the executable with Azure Artifact Signing;
6. archives it with `scripts/archive-release.ps1`, so `release.json` records the signed file;
7. builds the setup program from that archive and signs it too;
8. confirms both signatures are valid and writes `SHA256SUMS.txt`;
9. drafts a GitHub release with the setup program, its checksum, the install guide (`docs/INSTALL.md`) and this version's `CHANGELOG.md` entry.

The release archive, including the source ZIP, is kept as a workflow artifact (90 days by default); it is not attached to the public release.

Until signing is configured, the run still produces an **unsigned** draft marked as a test build. Don't publish that draft to other people.

## One-time Azure setup (about 30 minutes, plus identity checks that can take a few days)

Azure Artifact Signing (formerly *Trusted Signing*) costs about US$10/month on the Basic tier. Individual developers in Canada and the US can use it. Microsoft's walkthrough is at https://learn.microsoft.com/azure/artifact-signing/quickstart.

1. **Azure subscription.** Sign in at https://portal.azure.com and create a pay-as-you-go subscription if you don't have one.
2. **Turn on the service.** Go to *Subscriptions → your subscription → Resource providers*, then find and register **Microsoft.CodeSigning**.
3. **Create the signing account.** Search the portal for *Artifact Signing accounts* and choose **Create**. Pick a resource group, a region near you, a name (for example `budget-calendar-signing`) and the **Basic** tier. When it is created, open it and copy its **Account URI** from *Overview* (it looks like `https://<region>.codesigning.azure.net/`).
4. **Prove your identity.** In the signing account, go to *Access control (IAM)* and give *your own user* the **Artifact Signing Identity Verifier** role. Then go to *Identity validations → New identity → Public → Individual* and complete the check. Use your legal name: it becomes the *Publisher* that Windows shows people.
5. **Create the certificate profile.** When the identity shows *Completed*, go to *Certificate profiles → Create → Public Trust*, choose the validated identity and name it, for example `budget-calendar`.
6. **Give GitHub permission to sign, without storing a password.**
   1. Go to *Microsoft Entra ID → App registrations → New registration*, name it `budget-calendar-github-signing` and register it. Copy its **Application (client) ID** and **Directory (tenant) ID**.
   2. In that app, go to *Certificates & secrets → Federated credentials → Add credential*. Choose the scenario **GitHub Actions deploying Azure resources**. Set organization `davidwilliams-crypto`, repository `budget-calendar`, entity type **Environment** and environment name `release`.
   3. Back in the signing account, go to *Access control (IAM) → Add role assignment*. Choose **Artifact Signing Certificate Profile Signer**, search for `budget-calendar-github-signing`, select it, then **Review + assign**.
7. Copy your **Subscription ID** from *Subscriptions*.

## One-time GitHub setup (about 5 minutes)

In the repository, go to **Settings → Environments → New environment** and name it `release` (the federated credential trusts only this name). Then:

- Under **Deployment branches and tags**, choose *Selected branches and tags* and add the tag rule `v*`. Only version tags can sign.
  A manual *Run workflow* from a branch is then refused at the signing step, which is intended.
- Optionally, tick **Required reviewers** and add yourself, so each signing waits for your click.
- Under **Environment variables**, add the six values below. None of them is a password, so they are variables, not secrets:

| Variable | Value |
| --- | --- |
| `AZURE_CLIENT_ID` | Application (client) ID from step 6.1 |
| `AZURE_TENANT_ID` | Directory (tenant) ID from step 6.1 |
| `AZURE_SUBSCRIPTION_ID` | Subscription ID from step 7 |
| `AZURE_SIGNING_ENDPOINT` | Account URI from step 3 |
| `AZURE_SIGNING_ACCOUNT` | Signing account name from step 3 |
| `AZURE_CERTIFICATE_PROFILE` | Certificate profile name from step 5 |

Signing switches on as soon as `AZURE_SIGNING_ENDPOINT` is set.

*Optional:* to publish downloads in a different public repository, also set `RELEASE_REPOSITORY` (`owner/name`). Then add an environment **secret** `RELEASE_TOKEN`: a fine-grained token with *Contents: Read and write* on that repository only. The target repository needs at least one commit.

## Each release

1. Make the release changes as `AGENTS.md` describes: bump the version, add the `CHANGELOG.md` entry and pass the release gate. Merge them to `master`.
2. Tag the merged commit and push the tag:
   ```
   git tag v0.11.0
   git push origin v0.11.0
   ```
3. Watch **Actions → Release**. The full gate makes a run take a while. If it fails, fix the cause in a new version; never move a published tag.
4. Open **Releases** and find the new *Draft*. Download the setup program, install it on a Windows PC (ideally one that has never had Budget Calendar) and check that it starts. Then click **Publish release**.
5. Send people this link. It always points at the newest published version:
   `https://github.com/davidwilliams-crypto/budget-calendar/releases/latest`

## What people will see

A signed installer shows your name as the *Publisher*. SmartScreen also weighs how many people have downloaded a file. For the first releases, people may still see **"Windows protected your PC"**, now with your name instead of *Unknown publisher*. The install guide tells them to check that name and click **More info → Run anyway**. The warning fades as downloads accumulate.

The uninstaller that the setup program writes is not separately signed. Windows doesn't run SmartScreen on it.
