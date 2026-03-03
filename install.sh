#!/usr/bin/env bash
set -euo pipefail

# OxiProxy 安装 / 更新脚本
# 从 GitHub Releases 下载并安装 node / client / controller 二进制文件
# 支持多个镜像源，国内服务器无法连接 GitHub 时自动回退

REPO_OWNER="oxiproxy"
REPO_NAME="oxiproxy"
DEFAULT_DIR="/opt/oxiproxy"
INSTALL_DIR=""
COMPONENTS=()
VERSION=""
MODE=""
ALL_COMPONENTS=(controller node client)

# 下载源：名称 | API 地址模板 | 文件下载地址模板
# 模板变量: {owner} {repo} {version} {file}
MIRRORS=(
    "GitHub|https://api.github.com/repos/{owner}/{repo}/releases/latest|https://github.com/{owner}/{repo}/releases/download/{version}/{file}"
    "ghfast.top|https://api.github.com/repos/{owner}/{repo}/releases/latest|https://ghfast.top/https://github.com/{owner}/{repo}/releases/download/{version}/{file}"
    "gh-proxy.com|https://api.github.com/repos/{owner}/{repo}/releases/latest|https://gh-proxy.com/https://github.com/{owner}/{repo}/releases/download/{version}/{file}"
    "ghproxy.cc|https://api.github.com/repos/{owner}/{repo}/releases/latest|https://ghproxy.cc/https://github.com/{owner}/{repo}/releases/download/{version}/{file}"
)
MIRROR_IDX=0  # 当前选中的镜像索引

# ─── 颜色输出 ───────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

# ─── Banner ────────────────────────────────────────────
show_banner() {
    echo -e "${CYAN}"
    cat <<'BANNER'
   ____       _ ____
  / __ \_  __(_) __ \_________ __  ____  __
 / / / / |/_/ / /_/ / ___/ __ \| |/_/ / / /
/ /_/ />  </ / ____/ /  / /_/ />  </ /_/ /
\____/_/|_/_/_/   /_/   \____/_/|_|\__, /
                                  /____/
BANNER
    echo -e "${NC}"
}

# ─── 镜像模板替换 ─────────────────────────────────────────
render_url() {
    local tpl="$1"
    echo "$tpl" \
        | sed "s|{owner}|${REPO_OWNER}|g" \
        | sed "s|{repo}|${REPO_NAME}|g" \
        | sed "s|{version}|${VERSION}|g" \
        | sed "s|{file}|${2:-}|g"
}

# 获取当前镜像名称
get_mirror_name() { echo "${MIRRORS[$MIRROR_IDX]}" | cut -d'|' -f1; }
# 获取当前镜像 API URL 模板
get_mirror_api()  { echo "${MIRRORS[$MIRROR_IDX]}" | cut -d'|' -f2; }
# 获取当前镜像下载 URL 模板
get_mirror_dl()   { echo "${MIRRORS[$MIRROR_IDX]}" | cut -d'|' -f3; }

# ─── 选择下载源 ──────────────────────────────────────────
select_mirror() {
    echo -e "${BOLD}选择下载源:${NC}"
    echo ""

    local idx=0
    for m in "${MIRRORS[@]}"; do
        ((++idx))
        local name="${m%%|*}"
        if [[ $idx -eq 1 ]]; then
            echo -e "  ${CYAN}${idx})${NC} ${name}"
        else
            echo -e "  ${CYAN}${idx})${NC} ${name}  ${DIM}(镜像加速)${NC}"
        fi
    done

    ((++idx))
    echo -e "  ${CYAN}${idx})${NC} 自动检测 ${DIM}(逐个尝试，选最快的)${NC}"
    echo ""

    local choice
    read -rp "$(echo -e "${BOLD}输入编号${NC} ${DIM}[默认: ${idx} 自动检测]${NC}${BOLD}: ${NC}")" choice </dev/tty
    choice="${choice:-$idx}"

    # 自动检测
    if [[ "$choice" -eq "$idx" ]] 2>/dev/null; then
        auto_detect_mirror
        return
    fi

    local i=$((choice - 1))
    if [[ $i -ge 0 && $i -lt ${#MIRRORS[@]} ]] 2>/dev/null; then
        MIRROR_IDX=$i
    else
        warn "无效选项，使用自动检测"
        auto_detect_mirror
        return
    fi

    info "使用下载源: $(get_mirror_name)"
}

# ─── 自动检测可用镜像 ────────────────────────────────────
auto_detect_mirror() {
    info "正在检测可用的下载源..."

    for i in "${!MIRRORS[@]}"; do
        local name
        name=$(echo "${MIRRORS[$i]}" | cut -d'|' -f1)
        local api_tpl
        api_tpl=$(echo "${MIRRORS[$i]}" | cut -d'|' -f2)
        local test_url
        test_url=$(echo "$api_tpl" | sed "s|{owner}|${REPO_OWNER}|g" | sed "s|{repo}|${REPO_NAME}|g")

        printf "  测试 %-16s ... " "$name"

        if curl -fsSL --connect-timeout 5 --max-time 10 "$test_url" >/dev/null 2>&1; then
            echo -e "${GREEN}可用${NC}"
            MIRROR_IDX=$i
            info "使用下载源: ${name}"
            return
        else
            echo -e "${RED}不可用${NC}"
        fi
    done

    error "所有下载源均不可用，请检查网络连接"
}

# ─── 获取已安装组件的版本 ─────────────────────────────────
get_installed_version() {
    local bin="$1"
    if [[ -x "$bin" ]]; then
        "$bin" --version 2>/dev/null | head -1 || echo "未知版本"
    else
        echo ""
    fi
}

# ─── 检测已安装的组件 ─────────────────────────────────────
detect_installed() {
    local dir="$1"
    local found=0

    for comp in "${ALL_COMPONENTS[@]}"; do
        if [[ -x "${dir}/${comp}" ]]; then
            ((++found))
        fi
    done

    return $(( found == 0 ))
}

# ─── 选择操作模式 ─────────────────────────────────────────
select_mode() {
    echo -e "${BOLD}请选择操作:${NC}"
    echo ""
    echo -e "  ${CYAN}1)${NC} 全新安装"
    echo -e "  ${CYAN}2)${NC} 更新已安装的组件"
    echo ""

    local choice
    read -rp "$(echo -e "${BOLD}输入编号: ${NC}")" choice </dev/tty

    case "$choice" in
        1) MODE="install" ;;
        2) MODE="update" ;;
        *) error "无效选项" ;;
    esac
}

# ─── 选择组件（全新安装） ──────────────────────────────────
select_components() {
    echo -e "${BOLD}请选择要安装的组件:${NC}"
    echo ""
    echo -e "  ${CYAN}1)${NC} controller  - 控制器（Web 管理界面 + gRPC 服务）"
    echo -e "  ${CYAN}2)${NC} node        - 节点服务器（提供隧道服务）"
    echo -e "  ${CYAN}3)${NC} client      - 客户端（连接隧道）"
    echo -e "  ${CYAN}4)${NC} 全部安装"
    echo ""

    local choices
    read -rp "$(echo -e "${BOLD}输入编号（多选用空格分隔，如 1 2）: ${NC}")" choices </dev/tty

    [[ -z "$choices" ]] && error "未选择任何组件"

    for choice in $choices; do
        case "$choice" in
            1) COMPONENTS+=("controller") ;;
            2) COMPONENTS+=("node") ;;
            3) COMPONENTS+=("client") ;;
            4) COMPONENTS=(controller node client); break ;;
            *) warn "忽略无效选项: $choice" ;;
        esac
    done

    # 去重
    local -A seen; local unique=()
    for c in "${COMPONENTS[@]}"; do
        [[ -z "${seen[$c]:-}" ]] && seen[$c]=1 && unique+=("$c")
    done
    COMPONENTS=("${unique[@]}")

    if [[ ${#COMPONENTS[@]} -eq 0 ]]; then
        error "未选择任何有效组件"
    fi
}

# ─── 选择要更新的组件（基于已安装） ──────────────────────────
select_update_components() {
    local dir="$1"
    local installed=()
    local versions=()

    echo -e "${BOLD}已安装的组件:${NC}"
    echo ""

    local idx=0
    for comp in "${ALL_COMPONENTS[@]}"; do
        local bin="${dir}/${comp}"
        if [[ -x "$bin" ]]; then
            ((++idx))
            installed+=("$comp")
            local ver
            ver=$(get_installed_version "$bin")
            versions+=("$ver")
            echo -e "  ${CYAN}${idx})${NC} ${comp}  ${DIM}(当前: ${ver})${NC}"
        fi
    done

    if [[ ${#installed[@]} -eq 0 ]]; then
        warn "在 ${dir} 中未找到已安装的组件"
        return 1
    fi

    if [[ ${#installed[@]} -gt 1 ]]; then
        ((++idx))
        echo -e "  ${CYAN}${idx})${NC} 全部更新"
    fi
    echo ""

    local choices
    read -rp "$(echo -e "${BOLD}选择要更新的组件: ${NC}")" choices </dev/tty

    [[ -z "$choices" ]] && error "未选择任何组件"

    for choice in $choices; do
        # "全部更新" 选项
        if [[ "$choice" -eq $idx && ${#installed[@]} -gt 1 ]] 2>/dev/null; then
            COMPONENTS=("${installed[@]}")
            break
        fi

        local i=$((choice - 1))
        if [[ $i -ge 0 && $i -lt ${#installed[@]} ]] 2>/dev/null; then
            COMPONENTS+=("${installed[$i]}")
        else
            warn "忽略无效选项: $choice"
        fi
    done

    # 去重
    local -A seen; local unique=()
    for c in "${COMPONENTS[@]}"; do
        [[ -z "${seen[$c]:-}" ]] && seen[$c]=1 && unique+=("$c")
    done
    COMPONENTS=("${unique[@]}")

    if [[ ${#COMPONENTS[@]} -eq 0 ]]; then
        error "未选择任何有效组件"
    fi
}

# ─── 输入安装目录 ──────────────────────────────────────
select_install_dir() {
    echo ""
    read -rp "$(echo -e "${BOLD}安装目录${NC} ${DIM}[默认: ${DEFAULT_DIR}]${NC}${BOLD}: ${NC}")" input_dir </dev/tty
    INSTALL_DIR="${input_dir:-$DEFAULT_DIR}"
}

# ─── 输入版本 ─────────────────────────────────────────
select_version() {
    echo ""
    read -rp "$(echo -e "${BOLD}指定版本${NC} ${DIM}[留空使用最新版]${NC}${BOLD}: ${NC}")" input_version </dev/tty
    VERSION="${input_version}"
}

# ─── 检测系统架构 ──────────────────────────────────────
detect_target() {
    local os arch

    case "$(uname -s)" in
        Linux*)  os="unknown-linux-gnu" ;;
        Darwin*) os="apple-darwin" ;;
        *)       error "不支持的操作系统: $(uname -s)" ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64)   arch="x86_64" ;;
        aarch64|arm64)  arch="aarch64" ;;
        *)              error "不支持的架构: $(uname -m)" ;;
    esac

    echo "${arch}-${os}"
}

# ─── 获取最新版本号 ─────────────────────────────────────
get_latest_version() {
    local api_tpl
    api_tpl=$(get_mirror_api)
    local api_url
    api_url=$(render_url "$api_tpl")
    local version

    if command -v curl &>/dev/null; then
        version=$(curl -fsSL "$api_url" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')
    elif command -v wget &>/dev/null; then
        version=$(wget -qO- "$api_url" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')
    else
        error "需要 curl 或 wget"
    fi

    [[ -z "$version" ]] && error "无法获取最新版本号，请检查网络连接"
    echo "$version"
}

# ─── 下载文件 ──────────────────────────────────────────
download() {
    local url="$1" dest="$2"

    if command -v curl &>/dev/null; then
        curl -fSL --progress-bar -o "$dest" "$url"
    elif command -v wget &>/dev/null; then
        wget -q --show-progress -O "$dest" "$url"
    else
        error "需要 curl 或 wget"
    fi
}

# ─── 安装单个组件 ──────────────────────────────────────
install_component() {
    local component="$1" target="$2" version="$3"
    local archive_name="${component}-${version}-${target}.tar.gz"
    local dl_tpl
    dl_tpl=$(get_mirror_dl)
    local download_url
    download_url=$(render_url "$dl_tpl" "$archive_name")

    info "下载 ${component} ${version} (${target})..."
    info "URL: ${download_url}"

    local tmp_dir
    tmp_dir=$(mktemp -d)
    trap "rm -rf '$tmp_dir'" RETURN

    download "$download_url" "${tmp_dir}/${archive_name}" \
        || error "下载失败: ${download_url}\n请检查版本号和网络连接"

    info "解压中..."
    tar -xzf "${tmp_dir}/${archive_name}" -C "$tmp_dir"

    local binary="${tmp_dir}/${component}"
    [[ ! -f "$binary" ]] && binary=$(find "$tmp_dir" -name "$component" -type f | head -1)
    [[ -z "$binary" || ! -f "$binary" ]] && error "解压后未找到 ${component} 二进制文件"

    mkdir -p "$INSTALL_DIR"
    cp -f "$binary" "${INSTALL_DIR}/${component}"
    chmod +x "${INSTALL_DIR}/${component}"

    info "${component} 已安装到 ${INSTALL_DIR}/${component}"

    local ver_output
    ver_output=$("${INSTALL_DIR}/${component}" --version 2>/dev/null || true)
    if [[ -n "$ver_output" ]]; then
        info "版本: ${ver_output}"
    fi
}

# ─── 确认操作 ──────────────────────────────────────────
confirm() {
    local action="$1" target="$2"

    echo ""
    echo -e "${BOLD}────────────────────────────────${NC}"
    echo -e "  操    作: ${CYAN}${action}${NC}"
    echo -e "  下载源  : ${CYAN}$(get_mirror_name)${NC}"
    echo -e "  系统架构: ${CYAN}${target}${NC}"
    echo -e "  目标版本: ${CYAN}${VERSION}${NC}"
    echo -e "  安装目录: ${CYAN}${INSTALL_DIR}${NC}"
    echo -e "  组    件: ${CYAN}${COMPONENTS[*]}${NC}"
    echo -e "${BOLD}────────────────────────────────${NC}"
    echo ""

    read -rp "$(echo -e "${BOLD}确认？${NC} ${DIM}[Y/n]${NC} ")" yn </dev/tty
    case "${yn:-Y}" in
        [Yy]*) ;;
        *)     echo "已取消"; exit 0 ;;
    esac
}

# ─── 执行安装/更新 ────────────────────────────────────────
do_install() {
    local target="$1"

    echo ""
    local success=0 failed=0
    for component in "${COMPONENTS[@]}"; do
        if install_component "$component" "$target" "$VERSION"; then
            ((++success))
        else
            ((++failed))
            warn "${component} 安装失败"
        fi
        echo ""
    done

    if [[ $failed -eq 0 ]]; then
        echo -e "${GREEN}${BOLD}✓ 全部 ${success} 个组件操作成功！${NC}"
    else
        warn "${success} 个成功，${failed} 个失败"
    fi

    if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
        echo ""
        warn "${INSTALL_DIR} 不在 PATH 中，可执行:"
        echo -e "  ${CYAN}export PATH=\"${INSTALL_DIR}:\$PATH\"${NC}"
    fi
}

# ─── 主流程 ────────────────────────────────────────────
main() {
    show_banner

    select_mode
    echo ""

    select_mirror
    echo ""

    case "$MODE" in
        install)
            select_components
            select_install_dir
            select_version

            local target
            target=$(detect_target)
            info "系统架构: ${target}"

            if [[ -z "$VERSION" ]]; then
                info "获取最新版本..."
                VERSION=$(get_latest_version)
            fi

            confirm "全新安装" "$target"
            do_install "$target"
            ;;

        update)
            read -rp "$(echo -e "${BOLD}已安装目录${NC} ${DIM}[默认: ${DEFAULT_DIR}]${NC}${BOLD}: ${NC}")" input_dir </dev/tty
            INSTALL_DIR="${input_dir:-$DEFAULT_DIR}"

            if ! detect_installed "$INSTALL_DIR"; then
                error "在 ${INSTALL_DIR} 中未找到任何已安装的 OxiProxy 组件"
            fi

            echo ""
            select_update_components "$INSTALL_DIR"

            local target
            target=$(detect_target)

            echo ""
            info "获取最新版本..."
            VERSION=$(get_latest_version)
            info "最新版本: ${VERSION}"

            confirm "更新" "$target"
            do_install "$target"
            ;;
    esac
}

main
