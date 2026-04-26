import React from "react";
import {pushToast, type ToastKind} from "../lib/toast";
import {openMsgBoxWindow} from "../lib/msgbox";

function sendToast(kind: ToastKind) {
    const textMap: Record<ToastKind, string> = {
        info: "这是一条信息提示",
        success: "操作成功，状态已更新",
        error: "操作失败，请检查输入或重试",
    };
    pushToast(textMap[kind], kind, 2200);
}

export default function FrontendTestPage() {
    const [msgboxMessage, setMsgboxMessage] = React.useState("这是一个前端测试用消息框。");
    const [msgboxTitle, setMsgboxTitle] = React.useState("前端测试");

    const openConfirmMsgBox = async () => {
        await openMsgBoxWindow({
            id: `frontend-test-${Date.now()}`,
            title: msgboxTitle.trim() || "前端测试",
            message: msgboxMessage.trim() || "这是一个前端测试用消息框。",
            okText: "确认",
            cancelText: "取消",
        });
    };

    const openAlertMsgBox = async () => {
        await openMsgBoxWindow({
            id: `frontend-test-alert-${Date.now()}`,
            title: msgboxTitle.trim() || "前端测试",
            message: msgboxMessage.trim() || "这是一个前端测试用消息框。",
            okText: "知道了",
        });
    };

    return (
        <div className="diag-wrap">
            <section className="mj-panel card" style={{display: "grid", gap: 12}}>
                <h3 style={{margin: 0}}>Toast 测试</h3>
                <div style={{display: "flex", gap: 10, flexWrap: "wrap"}}>
                    <button className="btn" onClick={() => sendToast("info")}>触发 Info Toast</button>
                    <button className="btn" onClick={() => sendToast("success")}>触发 Success Toast</button>
                    <button className="btn" onClick={() => sendToast("error")}>触发 Error Toast</button>
                    <button
                        className="btn"
                        onClick={() => pushToast(`随机提示 @ ${new Date().toLocaleTimeString()}`, "info", 2800)}
                    >
                        触发带时间戳 Toast
                    </button>
                </div>
            </section>

            <section className="mj-panel card" style={{display: "grid", gap: 12}}>
                <h3 style={{margin: 0}}>MsgBox 测试</h3>
                <div style={{display: "grid", gap: 8, maxWidth: 720}}>
                    <label style={{display: "grid", gap: 6}}>
                        <span>标题</span>
                        <input
                            className="form-input"
                            value={msgboxTitle}
                            onChange={(e) => setMsgboxTitle(e.target.value)}
                        />
                    </label>
                    <label style={{display: "grid", gap: 6}}>
                        <span>内容</span>
                        <textarea
                            className="form-input"
                            rows={4}
                            value={msgboxMessage}
                            onChange={(e) => setMsgboxMessage(e.target.value)}
                        />
                    </label>
                </div>
                <div style={{display: "flex", gap: 10, flexWrap: "wrap"}}>
                    <button className="btn" onClick={openConfirmMsgBox}>打开确认框（OK/Cancel）</button>
                    <button className="btn" onClick={openAlertMsgBox}>打开提示框（仅 OK）</button>
                </div>
            </section>
        </div>
    );
}
