# Recovery key copy follow-up

The user needs to unlock the household shared from Windows and cannot find the recovery key they saved. Version 0.8.0 retains the key in the open document and Windows-protected cache but removes its copy control after creation. This is a small completion fix within the approved household onboarding work.

Add a Copy recovery key button to the current shared document section of File > Shared budget. Copy the active document's key on an explicit click and show generic feedback. Do not reveal it on screen, write plaintext files, regenerate the key, create a household, send a network request, or change the budget. Do not use the temporary unlock/create input or newly signed-in account to choose the key.

- Verify with generated fixtures that nothing is revealed or copied before a click, the active key is copied despite different temporary/account values, document and protected cache remain unchanged, and closing the document removes the control.
- Deliver 0.8.1 with matching lockfile/changelog, full workspace tests, standalone build-fingerprint tests, archive script tests, release CLI checks, and a new immutable archive.
- Keep the running 0.8.0 app and personal files intact. After release, the user can finish or save drafts and exit the old window; 0.8.1 can then open its existing protected shared cache and provide the key directly to the user's clipboard.
- Continue browser unlock and Android household invitation after the user privately pastes the key. Never include the key in chat or verification output.
