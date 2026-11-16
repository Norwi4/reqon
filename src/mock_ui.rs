use crate::{
    app::App,
    mock::{Routes, serve},
};
use tokio::net::TcpListener;

impl App {
    pub fn start_mock(&mut self) {
        let listener = match std::net::TcpListener::bind(("127.0.0.1", self.mock_port)) {
            Ok(l) => l,
            Err(e) => {
                self.notice(format!("Cannot start server: {e}"));
                return;
            }
        };
        if let Err(e) = listener.set_nonblocking(true) {
            self.notice(e.to_string());
            return;
        }
        let mut routes = Routes::new();
        for h in &self.ws.history {
            if h.reply.status > 0 && h.reply.error.is_empty() {
                if let Ok(url) = url::Url::parse(&h.reply.url) {
                    routes.insert(
                        format!("{} {}", h.request.method, url.path()),
                        h.reply.clone(),
                    );
                }
            }
        }
        let count = routes.len();
        let task = self.runtime.spawn(async move {
            if let Ok(listener) = TcpListener::from_std(listener) {
                serve(listener, routes, false).await;
            }
        });
        self.mock_task = Some(task.abort_handle());
        self.notice(format!("Mock server started with {count} recorded routes"));
    }
}
