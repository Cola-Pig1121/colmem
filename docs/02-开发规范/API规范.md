---
title: API规范
tags:
  - colmem/docs
  - colmem/api
---

# API规范

## CLI

- 所有 CLI 都以 `colmem` 为统一前缀。
- 读操作默认不修改状态。
- 写操作要明确语义，例如 `facts add`、`facts update`、`facts invalidate`。

## MCP

- 工具名统一使用 `colmem_*`。
- `tools/list` 必须暴露 `inputSchema`。
- `tools/call` 优先返回 `structuredContent`，文本只做兜底。
- 非法参数返回 `-32602`。
- 缺少顶层 `method` 返回 `-32600`。

## 时间与 facts

- `reference_date` 统一使用 `YYYY-MM-DD`。
- facts 查询允许 `active`、`history`、`scheduled`、`all` 四种 scope。
- facts 的生命周期必须可审计。

## 输出风格

- JSON 结构优先稳定，不追求一次性塞太多字段。
- 一旦字段进入 `structuredContent`，后续只能扩展，不能轻易改名。
