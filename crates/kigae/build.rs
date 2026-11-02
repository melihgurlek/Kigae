fn main() {
    #[cfg(feature = "overlay")]
    {
        let builder = cxx_qt_build::CxxQtBuilder::new_qml_module(
            cxx_qt_build::QmlModule::new("dev.kigae").qml_files(["qml/Overlay.qml", "qml/RiceCard.qml"]),
        )
        .qt_module("Quick")
        .files(["src/overlay.rs"]);
        // SAFETY: only silences GCC 16's -Wsfinae-incomplete, which Qt's own headers trip.
        let builder = unsafe {
            builder.cc_builder(|cc| {
                cc.flag_if_supported("-Wno-sfinae-incomplete");
            })
        };
        builder.build();
    }
}
