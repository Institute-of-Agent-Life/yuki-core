# 当前群的记忆陈述

原版 v4 的 `memory_change` 接受 `operation=create`、`target.scope_type=group`、`target.subject_ref=current_group`。目标来自原消息的有效群入口，证据保留原发言人。普通私聊、说话人冒充群、用群指代个人，以及其他群的证据均应拒绝。相同 key 的后续群陈述独立保存，原操作重读返回自己的回执。

CORE-026 的本地对拍使用真实群入站、模型工具调用和独立数据库。群事实的首次证据保存 `group_statement`，由此可读回 `group_report` 与 `automatic`。reader 55 为此前的证据保留空关系；旧事实仍按原有范围读取。群关系和证据读取继续受原会话及忘记规则约束。
