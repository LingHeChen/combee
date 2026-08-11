# Combee 服务器告警(飞书)

轻量告警方案:服务器 cron 每 5 分钟跑 `check.py`,检查服务/磁盘/证书/错误率,超阈值发飞书;每天 09:00 跑 `daily.py` 发日报。

## 一、建 3 个飞书群 + 3 个机器人

| 群 | 用途 | 收到 |
|---|---|---|
| **运维告警群** | P0/P1 紧急 | 服务挂、磁盘、证书、5xx |
| **提醒群** | P2 低优 | 资源水位 |
| **日报群** | 每日汇总 | 请求量/错误率/服务状态 |

每个群添加自定义机器人(飞书:群设置 → 群机器人 → 添加机器人 → 自定义机器人 → 复制 Webhook 地址,形如 `https://open.feishu.cn/open-apis/bot/v2/hook/xxx`)。

## 二、配置

```bash
cd /opt/combee/deploy/alert
cp config.example.env config.env
vim config.env
# 填入 3 个 webhook + 按需调阈值
```

## 三、部署到服务器 + 定时任务

```bash
# 1. 上传 deploy/alert/ 到服务器(随 deploy 目录一起)
# 2. 试跑(不发消息,只看会发什么)
python3 check.py --dry-run
python3 daily.py --dry-run

# 3. 正式跑一次(确认 webhook 通)
python3 check.py

# 4. 加 crontab
crontab -e
# 内容:
*/5 * * * *  cd /opt/combee/deploy/alert && python3 check.py >> /tmp/combee-alert.log 2>&1
0 9 * * *     cd /opt/combee/deploy/alert && python3 daily.py >> /tmp/combee-daily.log 2>&1
```

## 四、检查项与阈值(可在 config.env 调整)

| 检查 | 级别 | 默认阈值 |
|---|---|---|
| 服务副本不足(swarm) | P0 | 任何服务 < 期望副本 |
| 磁盘使用率 | P0 / P1 | 92% / 85% |
| 证书剩余天数 | P0 / P1 | 7 / 30 天 |
| 5xx 错误率(5 分钟窗口) | P0 / P1 | 10% / 2% |
| CPU / 内存水位 | P2 | 85% |

- **告警冷却**:同一项默认 30 分钟不重复(状态存 `/tmp/combee-alert-state.json`)。
- 日志检索基于 `docker service logs`,窗口 5 分钟;Swarm 环境需要 `stack_name` 正确。

## 五、后续升级(可选)

- 应用加 `/healthz` + `/metrics` 后,可升级到 **Prometheus + Alertmanager**(规则更灵活、支持静默/路由)。
- 备份失败告警(检查 COS 备份)可加进 `check.py`,等备份指标落地后补。

## 四、日志持久化 + 滚动(7 天)

`docker service logs` 不持久化(容器被滚动替换后旧日志从聚合里消失)。日志实际落在宿主
`/var/lib/docker/containers/<id>/<id>-json.log`(Docker 默认 json-file driver)。

1. 安装 logrotate 配置(滚动保留最近 7 天):

```bash
cp /opt/combee/deploy/alert/combee-logrotate /etc/logrotate.d/combee-containers
logrotate -d /etc/logrotate.d/combee-containers   # 试运行验证
```

2. check.py 已改为**直接读宿主机日志文件**(不依赖 docker service logs):
   - 滚动更新/容器重建后告警仍能读到日志;
   - 按 json-file 的 time 字段过滤最近 N 分钟(LOG_WINDOW_MIN);
   - 多副本自动遍历全部运行容器。

> 注意:logrotate 默认由 cron.daily 触发;若日志增长极快,可加 `maxsize 500M` 按大小提前滚动。
