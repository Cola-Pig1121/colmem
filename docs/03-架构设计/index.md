---
title: 架构设计
tags:
  - colmem/docs
  - colmem/architecture
---

# 架构设计

## 总览

`colmem` 是一个 host-agnostic 的本地 agent runtime，核心目标是统一记忆、facts、capabilities 和宿主接入。

## 子文档

- [[03-架构设计/目录结构]]
- [[03-架构设计/分包策略]]
- [[IMPLEMENTATION_PLAN]]

## 核心链路

1. ingest 原始记录
2. 切分 records/chunks
3. 建 full-text 与 vector 索引
4. 结合 facts 做 retrieval
5. 通过 harness 组装 context
6. 通过 CLI/MCP 暴露给宿主
