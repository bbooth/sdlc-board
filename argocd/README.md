# argocd

`application.yaml` points homelab-infra's ArgoCD at [`../deploy/`](../deploy/)
in the `sdlc-board` namespace. CI pushes `ghcr.io/bbooth/sdlc-board:latest`
from `main`; Image Updater commits the new digest and ArgoCD rolls the pod.

Cloudflare Access and the tunnel are in homelab-infra's `terraform/cloudflare`
(homelab-infra#230): `board.trulinc.com` routed to Traefik, an owner-only
Access application whose AUD is in `deploy/values.yaml`, and a service token.
On Omarchy, `terraform output -raw board_client_id` and `board_client_secret`
are the native app's `BOARD_ACCESS_CLIENT_ID` and `BOARD_ACCESS_CLIENT_SECRET`.

## One-time setup (the owner, by hand; nothing here is committed)

1. **Deploy keys**, as booking's `argocd/README.md` does: a read-only one
   in Secret `argocd/sdlc-board-repo` and a write one for Image Updater in
   `argocd/sdlc-board-image-updater-git`.
2. **Secrets** in namespace `sdlc-board`: `sdlc-board-tokens` with
   `LINEAR_API_KEY` and the GitHub App's settings (step 4), and
   `ghcr-pull` while the GHCR package is private. Without the App, an
   optional read-only `GITHUB_TOKEN` reads public repos only.
3. **The Application**, once the first image is pushed:
   `kubectl apply -f application.yaml`.
4. **The GitHub App**, so the board can read private repos' checks
   (fine-grained personal tokens can't). From a checkout on Omarchy,
   signed in to `gh` and with kubectl on the cluster
   (`~/.kube/k3s-homelab.yaml`):

   ```sh
   cargo run -p board-setup
   ```

   It opens a local page that sends GitHub the App's settings (read-only
   Checks, Contents, Metadata and Pull requests, no webhook, only this
   account). Confirm **Create GitHub App**, then on the install page
   choose **Only select repositories** and **Install**. It then makes the
   installation cover exactly `deploy/values.yaml`'s `repos`, adding and
   removing repos with your `gh` sign-in (if GitHub refuses, it says
   which to choose on the App's settings page), and does step 4.4 itself.
   The App's key never leaves memory. Once `sdlc-board-tokens` has
   `GITHUB_APP_ID`, it changes nothing.

   By hand instead:

   1. On GitHub, Settings > Developer settings > GitHub Apps > New GitHub
      App, under your own account. Any name and homepage URL. Untick
      **Webhook > Active**. Under **Repository permissions** set
      Checks, Contents, Metadata and Pull requests to **Read-only**, and
      leave everything else at No access. "Where can this GitHub App be
      installed?": Only on this account.
   2. Note the **App ID** on the App's page, then **Generate a private
      key**, which downloads a `.pem` file.
   3. **Install App** on your account, choosing **Only select
      repositories** and every repo in `deploy/values.yaml`'s `repos`
      (add new ones here when you onboard them).
   4. From a checkout on a machine whose kubectl reaches the cluster
      (`~/.kube/k3s-homelab.yaml` on Omarchy), hand the key to
      `board-setup` on stdin, then delete the file:

      ```sh
      cargo run -p board-setup -- key --app-id <App ID> < <downloaded>.pem \
        && rm <downloaded>.pem
      ```

      It checks the key parses and can sign, adds `GITHUB_APP_ID` and
      `GITHUB_APP_PRIVATE_KEY` to `sdlc-board-tokens` in one merge patch
      (keeping its other keys, and the PEM's line breaks), restarts the
      board and waits up to 2 minutes for the new pod to log
      `reading GitHub as the GitHub App`. Only then does it remove
      `GITHUB_TOKEN`, if the Secret has one. The key goes to kubectl on
      stdin only: never on a command line, into a file or the output.
      Run again once the Secret has `GITHUB_APP_ID`, it changes nothing.

      If the pod doesn't come up or doesn't sign in as the App, it
      removes both App keys and restarts the board, which goes back to
      `GITHUB_TOKEN` or tokenless reads (with `Recreate`, a pod that
      can't start is the board down), and exits non-zero naming the
      step that failed.

      To check the tokens, `kubectl -n sdlc-board port-forward
      deployment/sdlc-board 9091` and, in another terminal,
      `curl -s localhost:9091/metrics | grep board_github_app_tokens_total`:
      `result="ok"` counts up, and `result="error"` should stay at 0. A
      repo the App isn't installed on shows on the board as "the GitHub App
      isn't installed on ..." among the failed sources.

   5. **To back out** by hand, or to replace the App on purpose (then
      run step 4 again), remove both keys and restart:

      ```sh
      kubectl -n sdlc-board patch secret sdlc-board-tokens --type json \
        -p '[{"op":"remove","path":"/data/GITHUB_APP_ID"},{"op":"remove","path":"/data/GITHUB_APP_PRIVATE_KEY"}]' \
        && kubectl -n sdlc-board rollout restart deployment sdlc-board
      ```
