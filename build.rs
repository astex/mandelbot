use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("com.squeak.terminal")
            .qml_files(["qml/main.qml"]),
    )
    .file("src/ui/mod.rs")
    .build();
}
