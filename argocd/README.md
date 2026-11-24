# argocd

`application.yaml` points homelab-infra's ArgoCD at [`../deploy/`](../deploy/),
in the `sdlc-board` namespace. A merge to `main` makes CI push
`ghcr.io/bbooth/sdlc-board:latest`; Image Updater commits the new digest and
ArgoCD rolls the pod.

## One-time setup (the owner, by hand)

Nothing here is committed: keys and tokens live only in Cloudflare and the cluster.

1. **Cloudflare Access.** In Zero Trust, add a self-hosted application for
   `board.trulinc.com` with a policy allowing only your login, and a
   Service Auth policy allowing one service token for the native and
   Android apps. Put the team URL (`https://<team>.cloudflareaccess.com`)
   and the application's AUD tag in `deploy/values.yaml` under `access`.
   Keep the service token's ID and secret for the apps
   (`BOARD_ACCESS_CLIENT_ID`, `BOARD_ACCESS_CLIENT_SECRET`).
2. **The tunnel.** In homelab-infra, route `board.trulinc.com` to the
   Traefik Gateway like `booking.trulinc.com`.
3. **Repo access for ArgoCD and Image Updater**, as for booking:

   ```
   ssh-keygen -t ed25519 -N "" -C "argocd-sdlc-board" -f deploy-key
   gh repo deploy-key add deploy-key.pub --repo bbooth/sdlc-board --title "ArgoCD (read-only)"
   kubectl create secret generic sdlc-board-repo -n argocd \
     --from-literal=type=git \
     --from-literal=url=git@github.com:bbooth/sdlc-board.git \
     --from-file=sshPrivateKey=deploy-key
   kubectl label secret sdlc-board-repo -n argocd argocd.argoproj.io/secret-type=repository
   ssh-keygen -t ed25519 -N "" -C "argocd-image-updater-sdlc-board" -f iu-key
   kubectl -n argocd create secret generic sdlc-board-image-updater-git --from-file=sshPrivateKey=iu-key
   gh repo deploy-key add iu-key.pub --repo bbooth/sdlc-board --title "Image Updater" --allow-write
   rm deploy-key deploy-key.pub iu-key iu-key.pub
   ```

4. **The namespace's secrets.** The Linear key (and optionally a GitHub
   token, read-only, for checks within 2 minutes), and image pulls while
   the GHCR package is private:

   ```
   kubectl create namespace sdlc-board
   kubectl create secret generic sdlc-board-tokens -n sdlc-board \
     --from-literal=LINEAR_API_KEY=<key> --from-literal=GITHUB_TOKEN=<token>
   kubectl create secret docker-registry ghcr-pull -n sdlc-board \
     --docker-server=ghcr.io --docker-username=<github-username> \
     --docker-password=<PAT with read:packages>
   ```

5. **The Application**, once the first image is pushed:
   `kubectl apply -f application.yaml`.
