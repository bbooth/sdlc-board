# argocd

`application.yaml` points homelab-infra's ArgoCD at [`../deploy/`](../deploy/)
in the `sdlc-board` namespace. CI pushes `ghcr.io/bbooth/sdlc-board:latest`
from `main`; Image Updater commits the new digest and ArgoCD rolls the pod.

## One-time setup (the owner, by hand; nothing here is committed)

1. **Cloudflare Access:** a self-hosted application for `board.trulinc.com`
   allowing only your login, plus a Service Auth policy for one service
   token. Its team URL and AUD tag go in `deploy/values.yaml` (`access`);
   the token's ID and secret are the native app's `BOARD_ACCESS_CLIENT_ID`
   and `BOARD_ACCESS_CLIENT_SECRET`.
2. **The tunnel:** in homelab-infra, route `board.trulinc.com` to the
   Traefik Gateway like `booking.trulinc.com`.
3. **Deploy keys**, as booking's `argocd/README.md` does: a read-only one
   in Secret `argocd/sdlc-board-repo` and a write one for Image Updater in
   `argocd/sdlc-board-image-updater-git`.
4. **Secrets** in namespace `sdlc-board`: `sdlc-board-tokens` with
   `LINEAR_API_KEY` (and optionally a read-only `GITHUB_TOKEN`), and
   `ghcr-pull` while the GHCR package is private.
5. **The Application**, once the first image is pushed:
   `kubectl apply -f application.yaml`.
