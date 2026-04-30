FROM gcr.io/distroless/cc-debian12:nonroot
COPY omen /omen
ENTRYPOINT ["/omen"]
