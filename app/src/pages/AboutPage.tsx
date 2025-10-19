import React from "react";
import styles from "./AboutPage.module.css";

export default function AboutPage() {
    return (
        <div className={styles.wrap}>
            <h1>向听镜 <span className={styles.sub}>Shanten Lens</span></h1>

            <section>
                <h2>作者</h2>
                <p>海绵couna</p>
            </section>

            <section>
                <h2>版权与许可</h2>
                <p>
                    © {new Date().getFullYear()} 向听镜（Shanten Lens）。本项目采用
                    <strong> Apache License 2.0</strong>。
                </p>
                <details>
                    <summary>查看许可摘要</summary>
                    <pre className={styles.license}>
{`Licensed under the Apache License, Version 2.0 (the "License").
You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
Distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND.`}
          </pre>
                </details>
            </section>

            <section>
                <h2>素材来源</h2>
                <p>部分资源来源于「雀魂」。仅用于非商业用途与学习研究。</p>
            </section>

            <section>
                <h2>使用提醒</h2>
                <p>
                    为减小影响范围，请<strong>不要在公开场合传播或宣传</strong>本软件及其运行画面
                    （例如：直播、公开群聊、公开社媒等）。该提醒仅为使用建议，不构成对 Apache 许可的附加法律限制。
                </p>
            </section>

            <footer className={styles.footer}>
                <details>
                    <summary>仓库地址</summary>
                    <p className={styles.muted}>
                        <a
                            href="https://github.com/SpCoGov/shanten-lens"
                            target="_blank"
                            rel="noreferrer"
                        >
                            Github
                        </a>
                    </p>
                </details>
            </footer>
        </div>
    );
}