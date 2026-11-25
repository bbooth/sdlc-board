{{/* The distinct rollout namespaces of the repos, as a JSON array. */}}
{{- define "sdlc-board.rolloutNamespaces" -}}
{{- $out := list }}
{{- range . }}{{ with .rolloutNamespace }}{{ $out = append $out . }}{{ end }}{{ end }}
{{- $out | uniq | toJson }}
{{- end }}
