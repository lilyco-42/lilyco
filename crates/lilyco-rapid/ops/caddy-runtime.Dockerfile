FROM caddy:2.11.9-alpine
COPY ops/Caddyfile /etc/caddy/Caddyfile
COPY --from=build /src/dist /srv
EXPOSE 8080
