FROM vicanso/pingap:0.16.2-distroless
COPY ops/Pingap.toml /opt/pingap/rapid.toml
COPY --from=build /src/dist /srv
ENV PINGAP_CONF=/opt/pingap/rapid.toml
USER 65532:65532
EXPOSE 8080
