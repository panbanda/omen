FROM gcr.io/distroless/static-debian12:nonroot
COPY omen /omen
ENTRYPOINT ["/omen"]
