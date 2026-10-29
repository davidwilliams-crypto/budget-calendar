# Mobile household unlock follow-up

During the approved Android onboarding, the owner has two households and knows the desired household's recovery key. The current new-device flow always chooses the first household returned by the server, while the household picker is available only after unlocking. The owner cannot reach the intended household without another household's key.

Add a household selector to the unlock form, defaulting to the requested household and listing only the signed-in account's accessible households. Bind verification, saved key and subsequent selection to the chosen identity. Show which household is being unlocked even when only one exists. Preserve account/context fencing and never persist a key until decryption succeeds.

- Use generated browser fixtures to verify a fresh owner device with two households can unlock the second using only its key, wrong keys do not persist under either identity, and reload keeps the chosen household.
- Bump GUI/lock/changelog to 0.8.2 and rotate the PWA shell cache name. Run relevant mobile and browser regressions, full Rust workspace tests, standalone build fingerprint checks, archive checks, release CLI checks, and immutable Windows/mobile packaging.
- Push the tested source to the existing private feature branch and manually deploy the same existing Render service. Preserve its data volume and configuration; no additional service or spending is required.
- The user deferred the wife's phone installation. Continue the owner's phone setup when the update is live; invitation acceptance can be completed later.
