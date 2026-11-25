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
   `LINEAR_API_KEY` (and optionally a read-only `GITHUB_TOKEN`), and
   `ghcr-pull` while the GHCR package is private.
3. **The Application**, once the first image is pushed:
   `kubectl apply -f application.yaml`.
