# docker build -t bench-upstream bench/upstream
FROM docker.io/library/golang:1.22-bookworm AS build
WORKDIR /src
COPY go.mod main.go ./
RUN CGO_ENABLED=0 go build -o /upstream .

FROM scratch
COPY --from=build /upstream /upstream
ENTRYPOINT ["/upstream"]
