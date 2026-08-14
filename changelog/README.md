# changelog — 发版说明

**构建规则:只有本目录出现新文件(新版本)时,CI 才自动编译**。
平时改 `src/`、README 等都不会触发编译。

## 发版流程

1. 改完代码,准备发版
2. 新建 `changelog/vX.Y.Z.md`(照着 `v0.1.0.md` 的格式写这一版改了什么)
3. 提交并 push 到 main → 自动触发三平台编译验证
4. 编译通过后**自动发布 GitHub Release**:tag = `vX.Y.Z`,正文 = 本文件内容,
   附件 = macOS `DSH.app` 压缩包 + Windows exe + Linux 二进制
5. 改完代码想立即验证 → GitHub → **Actions → Run workflow**(手动触发;已发过的版本会重建)

## 注意

- 只提交代码、不加 changelog → 不编译(这是约定,不是 bug)
- 想在同一个提交里"代码 + changelog 一起发版" → 没问题,照样触发
