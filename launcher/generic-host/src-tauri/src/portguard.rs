//! 端口闸门：判断「现在这一秒能不能真的绑上网关端口」，并在挡路的是能证明属于本项目的
//! 残留时先清掉、再复测。
//!
//! 分工：分类与「哪些进程能清」全在 `ports::BlockKind`，这里只管咨询、清理、复测，
//! 以及界面该收到什么事件与日志怎么写。
//!
//! 边界：玩家自己开着的客户端（own-client）与认不出名字的外部进程一律不动手；
//! 自动清理只做一次 reconcile，清完立刻复测，复测还挡着就照旧报错——
//! 绝不宣称一个没验证过的「已清干净」。

use serde::Serialize;

use crate::config;
use crate::paths::{Root, GATEWAY_PORT, SERVER_IMAGE_NAMES};
use crate::ports::{self, BlockKind, Owner};
use crate::procutil;

#[derive(Clone, Debug, Serialize)]
pub struct Leftover {
    pub pid: u32,
    pub image: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub gateway: Owner,
    pub leftovers: Vec<Leftover>,
    /// 挡路的占用者（现在只有网关端口一个）。
    pub blocked: Vec<Owner>,
    pub killable: Vec<u32>,
    pub protected_ones: Vec<Owner>,
    pub killed: Vec<u32>,
    pub failed: Vec<String>,
    pub rechecked: bool,
    pub summary: String,
}

impl Report {
    pub fn blocked_now(&self) -> bool {
        !self.blocked.is_empty()
    }
}

pub fn check(_root: &Root) -> Report {
    let gateway = ports::gateway_owner();
    let leftovers: Vec<Leftover> = procutil::running_named(&SERVER_IMAGE_NAMES)
        .into_iter()
        .map(|entry| Leftover {
            pid: entry.pid,
            image: entry.name,
        })
        .collect();

    let blocked: Vec<Owner> = if gateway.listening {
        vec![gateway.clone()]
    } else {
        Vec::new()
    };
    let killable: Vec<u32> = blocked
        .iter()
        .filter(|owner| owner.kind.auto_cleanable())
        .filter_map(|owner| owner.pid)
        .collect();
    let protected_ones: Vec<Owner> = blocked
        .iter()
        .filter(|owner| !owner.kind.auto_cleanable())
        .cloned()
        .collect();

    let summary = summarize(&gateway, &leftovers);
    Report {
        gateway,
        leftovers,
        blocked,
        killable,
        protected_ones,
        killed: Vec::new(),
        failed: Vec::new(),
        rechecked: false,
        summary,
    }
}

fn summarize(gateway: &Owner, leftovers: &[Leftover]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if gateway.listening {
        let who = match (gateway.image.as_deref(), gateway.pid) {
            (Some(image), Some(pid)) => format!("{image} · PID {pid}"),
            (Some(image), None) => image.to_string(),
            (None, Some(pid)) => format!("PID {pid}（进程名读不到，可能需要管理员权限）"),
            (None, None) => "未知进程".to_string(),
        };
        parts.push(format!(
            "网关端口 {GATEWAY_PORT} 已被监听：{}（{}）",
            who,
            gateway.kind.label()
        ));
    }
    if !leftovers.is_empty() {
        let names: Vec<String> = leftovers
            .iter()
            .map(|item| format!("{} · PID {}", item.image, item.pid))
            .collect();
        parts.push(format!(
            "另有 {} 个本项目残留进程：{}",
            leftovers.len(),
            names.join("、")
        ));
    }
    if parts.is_empty() {
        return "网关端口空闲，无本项目残留进程".to_string();
    }
    parts.join("；")
}

/// 清理提示：告诉玩家下一步能点什么。
pub fn cleanup_hint(root: &Root, report: &Report) -> String {
    if report.blocked.is_empty() {
        return String::new();
    }
    let owner = &report.blocked[0];
    match owner.kind {
        BlockKind::OwnClient => format!(
            "占着网关的是游戏客户端自己。请先关掉游戏窗口（{}），\
             或点「强制清理并继续」由启动器结束它。",
            config::client_dir(root)
                .join(crate::paths::CLIENT_IMAGE_NAME)
                .display()
        ),
        BlockKind::Unknown => format!(
            "占着网关的进程（{}）不属于本项目，启动器不会替你结束别人的程序。\
             请手动退出它，或改端口后重试。",
            owner
                .image
                .clone()
                .unwrap_or_else(|| "名字读不到".to_string())
        ),
        _ => "挡路的是上一轮没退干净的本项目进程，点「强制清理并继续」即可。".to_string(),
    }
}

/// 只清本项目自己的网关/编排残留，清完复测一次。返回 (新报告, 有没有真的动手)。
pub fn auto_clean_own(root: &Root) -> (Report, bool) {
    let report = check(root);
    if !report.blocked_now() {
        return (report, false);
    }
    // 挡路的只要有不是本项目残留的（玩家自己开着的客户端、外人进程、读不到名字的），
    // 就一个都不动 —— 那是玩家该做的决定，不是启动器替做的。
    if report.killable.is_empty() || !report.protected_ones.is_empty() {
        return (report, false);
    }
    let targets = report.killable.clone();
    let (killed, failed) = kill_pids(&targets);
    let mut fresh = check(root);
    fresh.killed = killed;
    fresh.failed = failed;
    fresh.rechecked = true;
    (fresh, true)
}

/// 界面上的「强制清理并继续」：把本项目所有网关/编排残留按 PID 与镜像名各收一遍，
/// 再复测。玩家自己的客户端与外部进程仍然不碰（那种情况请关游戏或改占用者）。
pub fn force_clean(root: &Root) -> Report {
    let report = check(root);
    let mut targets: Vec<u32> = report
        .leftovers
        .iter()
        .map(|item| item.pid)
        .chain(report.killable.iter().copied())
        .collect();
    targets.sort_unstable();
    targets.dedup();
    let (killed, failed) = kill_pids(&targets);
    for name in &SERVER_IMAGE_NAMES {
        for pid in procutil::pids_by_name(name) {
            if !targets.contains(&pid) {
                procutil::kill_tree(pid);
            }
        }
    }
    let mut fresh = check(root);
    fresh.killed = killed;
    fresh.failed = failed;
    fresh.rechecked = true;
    fresh
}

pub fn kill_pids(targets: &[u32]) -> (Vec<u32>, Vec<String>) {
    let mut killed = Vec::new();
    let mut failed = Vec::new();
    for pid in targets {
        if procutil::kill_tree(*pid) && !procutil::is_running(*pid) {
            killed.push(*pid);
        } else {
            let name = procutil::name_of(*pid).unwrap_or_else(|| "未知进程".to_string());
            failed.push(format!("{name} · PID {pid}"));
        }
    }
    (killed, failed)
}
