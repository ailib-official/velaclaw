# 第十五章：远程部署

本章介绍如何将 VelaClaw 部署到远程服务器。

---

## 目录

1. [部署概述](#部署概述)
2. [部署模式](#部署模式)
3. [快速部署](#快速部署)
4. [配置管理](#配置管理)
5. [监控与维护](#监控与维护)

---

## 部署概述

### 为什么需要远程部署？

| 本地运行 | 远程部署 |
|----------|----------|
| 占用本机资源 | 不占本机资源 |
| 电脑关机就停止 | 7×24 小时运行 |
| 只能自己用 | 可以多人共享 |
| 网络不稳定影响大 | 服务器网络更稳定 |

### 部署要求

**服务器要求**：

| 项目 | 最低要求 | 推荐配置 |
|------|----------|----------|
| CPU | 1 核 | 2 核+ |
| 内存 | 512MB | 2GB+ |
| 存储 | 1GB | 10GB+ |
| 系统 | Linux | Ubuntu 22.04 |

**网络要求**：
- 公网 IP（如需外网访问）
- 或内网穿透服务

---

## 部署模式

### 模式一：直接部署

**特点**：直接在服务器上运行二进制文件

**优点**：简单、资源占用少

**缺点**：依赖系统环境

**适合**：简单服务器、个人使用

### 模式二：Docker 部署

**特点**：使用容器运行

**优点**：环境隔离、易于迁移

**缺点**：需要 Docker 环境

**适合**：容器化环境、团队使用

### 模式三：系统服务

**特点**：注册为系统服务（systemd）

**优点**：自动启动、崩溃重启

**缺点**：配置复杂

**适合**：生产环境、长期运行

---

## 快速部署

### 步骤 1：配置服务器信息

编辑 `~/.velaclaw/deploy.yaml`：

```yaml
servers:
  - id: my-server
    host: 192.168.1.100  # 服务器 IP
    user: deploy         # SSH 用户名
    port: 22             # SSH 端口
    ssh_key: ~/.ssh/id_rsa  # SSH 密钥路径
    labels:
      env: production
```

### 步骤 2：配置部署选项

```yaml
deploy:
  mode: direct  # direct/docker/systemd
  
  binary_path: /usr/local/bin/velaclaw
  config_path: /etc/velaclaw/config.yaml
  working_dir: /var/lib/velaclaw
  
  auto_start: true
  restart_on_failure: true
  max_restarts: 3
```

### 步骤 3：执行部署

```bash
# 部署到服务器
velaclaw deploy my-server
```

输出：

```
部署进度
========

[1/5] 创建目录...          ✓
[2/5] 上传二进制文件...     ✓
[3/5] 上传配置文件...       ✓
[4/5] 设置权限...           ✓
[5/5] 启动服务...           ✓

部署成功！
服务器地址: 192.168.1.100
状态: 运行中
```

---

## 配置管理

### 同步配置到服务器

```bash
# 上传配置文件
velaclaw deploy sync-config my-server
```

### 查看服务器状态

```bash
velaclaw deploy status my-server
```

输出：

```
服务器状态
==========

服务器: my-server (192.168.1.100)
状态: ✓ 运行中
版本: 0.1.0
运行时间: 3 天 5 小时
内存使用: 15 MB
CPU 使用: 0.5%

渠道状态:
  Telegram: ✓ 运行
  Discord: ✓ 运行

最近日志:
  [INFO] 收到消息来自 user123
  [INFO] 处理完成，耗时 1.2s
```

### 更新服务器

```bash
# 更新到最新版本
velaclaw deploy update my-server

# 或指定版本
velaclaw deploy update my-server --version 0.2.0
```

---

## 监控与维护

### 健康检查

```bash
velaclaw deploy health-check my-server
```

输出：

```
健康检查
========

✓ 服务运行正常
✓ 内存使用正常 (15MB / 2GB)
✓ 磁盘空间充足 (50GB 可用)
✓ 网络连接正常
✓ API 密钥有效
✓ 渠道连接正常

状态: 健康
```

### 查看日志

```bash
# 实时查看日志
velaclaw deploy logs my-server --follow

# 查看最近日志
velaclaw deploy logs my-server --lines 100
```

### 重启服务

```bash
# 重启
velaclaw deploy restart my-server

# 停止
velaclaw deploy stop my-server

# 启动
velaclaw deploy start my-server
```

### 回滚

如果更新后出现问题：

```bash
velaclaw deploy rollback my-server
```

---

## Docker 部署

### 配置 Docker 模式

```yaml
deploy:
  mode: docker
  
  docker:
    image: velaclaw/velaclaw:latest
    ports:
      - "8080:8080"
    volumes:
      - ~/.velaclaw:/root/.velaclaw
    environment:
      - RUST_LOG=info
```

### 部署命令

```bash
velaclaw deploy my-server
```

### Docker 常用命令

```bash
# 查看容器状态
velaclaw deploy status my-server

# 查看容器日志
docker logs velaclaw

# 进入容器
docker exec -it velaclaw bash
```

---

## 系统服务部署

### 配置 systemd

```yaml
deploy:
  mode: systemd
  
  systemd:
    service_name: velaclaw
    description: VelaClaw AI Assistant
    user: velaclaw
    group: velaclaw
```

### 部署后管理

```bash
# 查看服务状态
ssh user@server "systemctl status velaclaw"

# 查看日志
ssh user@server "journalctl -u velaclaw -f"

# 手动重启
ssh user@server "systemctl restart velaclaw"
```

---

## 多服务器管理

### 配置多台服务器

```yaml
servers:
  - id: prod-1
    host: 192.168.1.100
    user: deploy
    labels:
      env: production
      region: cn-east
      
  - id: prod-2
    host: 192.168.1.101
    user: deploy
    labels:
      env: production
      region: cn-west
      
  - id: staging
    host: 192.168.1.200
    user: deploy
    labels:
      env: staging
```

### 批量部署

```bash
# 部署到所有生产服务器
velaclaw deploy --label env=production

# 部署到特定区域
velaclaw deploy --label region=cn-east
```

---

## 安全建议

### 1. 使用 SSH 密钥认证

```bash
# 生成密钥
ssh-keygen -t ed25519 -C "velaclaw-deploy"

# 复制公钥到服务器
ssh-copy-id -i ~/.ssh/velaclaw-deploy.pub user@server
```

### 2. 限制网络访问

```yaml
# 只允许特定 IP 访问
gateway:
  host: 127.0.0.1  # 只监听本地
  
  # 或使用防火墙规则
```

### 3. 定期更新

```bash
# 设置自动更新检查
velaclaw cron add "0 3 * * *" "EXEC: velaclaw deploy check-update --all"
```

### 4. 备份配置

```bash
# 定期备份
velaclaw cron add "0 2 * * *" "EXEC: velaclaw deploy backup --all"
```

---

## 故障排除

### 连接失败

```bash
# 测试 SSH 连接
ssh user@server "echo 连接成功"

# 检查密钥权限
chmod 600 ~/.ssh/id_rsa
```

### 服务无法启动

```bash
# 查看详细日志
velaclaw deploy logs my-server --debug

# 检查端口占用
ssh user@server "netstat -tlnp | grep 8080"
```

### 内存不足

```bash
# 检查内存使用
ssh user@server "free -h"

# 重启服务释放内存
velaclaw deploy restart my-server
```

---

## 下一步

1. **了解安全设置** → [安全设置](./17-security.md)
2. **查看命令参考** → [命令参考](./18-commands.md)
3. **解决常见问题** → [常见问题](./20-faq.md)

---

[← 上一章：自动化与定时任务](./14-automation.md) | [返回目录](./README.md) | [下一章：硬件外设 →](./16-hardware.md)
