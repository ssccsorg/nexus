# The demonstration as a container: the committed board, built and served, with nothing installed on the host.
#
#   docker build -t nexus-demo storage/ref/demo
#   docker run --rm -p 8080:8080 nexus-demo
#
# The board and the runs are committed, so the image needs no Rust toolchain: it builds the application and serves the
# files. Regenerating the board from a volume is a host step, where the reference tool and the volumes are.

FROM node:22-bookworm-slim AS board

# The frame is a sibling checkout on a host. An image has no sibling, so it is cloned, which is what `run.sh` does when a
# checkout has none.
RUN apt-get update \
 && apt-get install -y --no-install-recommends git ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 https://github.com/ssccsorg/skene /src/skene

# The application names the frame five levels up from itself, so it is copied to the depth that path expects.
COPY app /src/nexus/storage/ref/demo/app
WORKDIR /src/nexus/storage/ref/demo/app
RUN npm ci && npm run build

FROM nginx:1.27-alpine
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=board /src/nexus/storage/ref/demo/app/dist /usr/share/nginx/html
EXPOSE 8080
