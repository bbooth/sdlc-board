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
   (fine-grained personal tokens can't):
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
   4. Add both to the existing Secret in one patch, keeping its other
      keys, then restart the pod and check it came up. The key goes in as
      the file's bytes: the server needs the PEM's real line breaks, and a
      key pasted on one line with `\n` escapes fails at start. The patch
      goes through a pipe, so the key is never on a command line or in a
      file. Each command runs only if the one before worked, and
      `openssl` checks the key first, because a pod given an ID without a
      usable key exits at start, and with `Recreate` the board is down
      until it's fixed:

      ```sh
      openssl rsa -in <downloaded>.pem -check -noout \
        && kubectl create secret generic app --dry-run=client \
          --from-literal=GITHUB_APP_ID=<App ID> \
          --from-file=GITHUB_APP_PRIVATE_KEY=<downloaded>.pem \
          -o go-template='{"data":{"GITHUB_APP_ID":"{{.data.GITHUB_APP_ID}}","GITHUB_APP_PRIVATE_KEY":"{{.data.GITHUB_APP_PRIVATE_KEY}}"}}' \
        | kubectl -n sdlc-board patch secret sdlc-board-tokens --type merge --patch-file /dev/stdin \
        && kubectl -n sdlc-board rollout restart deployment sdlc-board \
        && kubectl -n sdlc-board rollout status deployment sdlc-board --timeout=2m \
        && kubectl -n sdlc-board logs deployment/sdlc-board | grep 'reading GitHub as the GitHub App' \
        && rm <downloaded>.pem
      ```

      To check the tokens, `kubectl -n sdlc-board port-forward
      deployment/sdlc-board 9091` and, in another terminal,
      `curl -s localhost:9091/metrics | grep board_github_app_tokens_total`:
      `result="ok"` counts up, and `result="error"` should stay at 0. A
      repo the App isn't installed on shows on the board as "the GitHub App
      isn't installed on ..." among the failed sources.

   5. **To back out**, if the pod doesn't come up or the App misbehaves,
      remove both keys and restart; the board goes back to `GITHUB_TOKEN`
      or tokenless reads:

      ```sh
      kubectl -n sdlc-board patch secret sdlc-board-tokens --type json \
        -p '[{"op":"remove","path":"/data/GITHUB_APP_ID"},{"op":"remove","path":"/data/GITHUB_APP_PRIVATE_KEY"}]' \
        && kubectl -n sdlc-board rollout restart deployment sdlc-board
      ```

   While the old `GITHUB_TOKEN` key is set too, it's ignored with a
   warning. Once the App works, remove it:

   ```sh
   kubectl -n sdlc-board patch secret sdlc-board-tokens --type json \
     -p '[{"op":"remove","path":"/data/GITHUB_TOKEN"}]'
   ```
