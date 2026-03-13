# 使用多阶段构建
# 阶段1: 构建前端
FROM oven/bun:alpine AS web-builder

WORKDIR /build/dashboard

# 复制前端依赖文件
COPY dashboard/package.json dashboard/bun.lock ./

# 安装依赖
RUN bun install --frozen-lockfile

# 复制前端源码
COPY dashboard/ ./

# 构建前端
RUN bun run build

# 阶段2: cargo-chef 基础镜像
FROM lukemathwalker/cargo-chef:latest-rust-1-alpine AS chef

# 安装编译依赖
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig protobuf-dev

WORKDIR /build

# 阶段3: 生成依赖 recipe
FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# 阶段4: 构建依赖（此层会被 Docker 缓存）
FROM chef AS builder

# 复制 recipe（仅包含依赖信息）
COPY --from=planner /build/recipe.json recipe.json

# 复制 build.rs 和 proto 文件（protobuf 代码生成需要）
COPY common/build.rs ./common/build.rs
COPY common/proto ./common/proto

# 构建依赖 - 只要 Cargo.lock 不变，此层就会被缓存
RUN cargo chef cook --release --recipe-path recipe.json

# 复制实际源码
COPY . .

# 复制前端构建产物到 dist 目录
COPY --from=web-builder /build/dist ./dist

# 构建项目代码（依赖已缓存，只编译项目自身代码）
RUN cargo build --release -p node -p client -p controller

# 阶段5: 最终镜像
FROM alpine:latest

# 安装运行时依赖
RUN apk add --no-cache libgcc ca-certificates && \
    addgroup -g 1000 oxiproxy && \
    adduser -D -u 1000 -G oxiproxy oxiproxy

WORKDIR /app

# 从构建阶段复制二进制文件
COPY --from=builder /build/target/release/node /app/
COPY --from=builder /build/target/release/client /app/
COPY --from=builder /build/target/release/controller /app/
COPY --from=builder /build/dist /app/dist

# 创建数据目录并设置权限
RUN mkdir -p /app/data && \
    chown -R oxiproxy:oxiproxy /app

# 切换到非特权用户
USER oxiproxy

# 暴露端口
EXPOSE 7000 3000 3100

# 默认运行 controller
# 可以通过 docker run 时指定不同的命令来运行 node 或 client
CMD ["/app/controller"]
