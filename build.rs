//! 构建脚本：Windows 下把 `imgs/app.ico` 嵌进 exe，作为程序 / 任务栏图标。

fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("imgs/app.ico");
        if let Err(e) = res.compile() {
            // 没装 Windows SDK 之类的环境里别让整个构建挂掉，只提示一下。
            println!("cargo:warning=嵌入图标失败: {e}");
        }
        println!("cargo:rerun-if-changed=imgs/app.ico");
    }
}
