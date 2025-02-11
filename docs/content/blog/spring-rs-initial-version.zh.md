+++
title = "spring-rs初始版本发布"
description = "经过一个月的沉淀，我用Rust编写了一个类似于spring-boot的微服务框架。下面是一个最简单的web应用的例子"
date = 2024-08-04T09:19:42+00:00
updated = 2024-08-04T09:19:42+00:00
draft = false
template = "blog/page.html"

[extra]
lead = "经过一个月的沉淀，我用Rust编写了一个类似于spring-boot的微服务框架。下面是一个最简单的web应用的例子"
+++

```rust
use spring::{route, get, App};
use spring_web::{
    extractor::Path, handler::TypeRouter, response::IntoResponse, 
    Router, WebConfigurator, WebPlugin,
};

#[auto_config(WebConfigurator)]
#[tokio::main]
async fn main() {
    App::new()
        .add_plugin(WebPlugin)
        .run()
        .await
}

#[get("/")]
async fn hello_world() -> impl IntoResponse {
    "hello world"
}

#[route("/hello/{name}", method = "GET", method = "POST")]
async fn hello(Path(name): Path<String>) -> impl IntoResponse {
    format!("hello {name}")
}
```

`spring-rs`使用插件的方式整合了Rust生态中流行的几个框架，并提供了过程宏来简化开发。

对`spring-rs`感兴趣的可以[点击这里](/zh/docs/getting-started/quick-start/)快速上手。
