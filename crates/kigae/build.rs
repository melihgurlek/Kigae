fn main() {
    #[cfg(feature = "overlay")]
    cxx_qt_build::CxxQtBuilder::new_qml_module(
        cxx_qt_build::QmlModule::new("dev.kigae").qml_files(["qml/Overlay.qml", "qml/RiceCard.qml"]),
    )
    .qt_module("Quick")
    .files(["src/overlay.rs"])
    .build();
}
