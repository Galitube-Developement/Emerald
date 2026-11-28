fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/emerald.ico");
        res.compile().expect("embed Windows icon");
    }
}
