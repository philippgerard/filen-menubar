// Regression coverage for the proc-macro-error removal backport. Compile actual
// invocations so parser errors remain compiler diagnostics rather than panics.

#[test]
fn invalid_inputs_report_compiler_errors() {
    let cases = trybuild2::TestCases::new();
    let invalid = [
        (
            "enum_requires_enum",
            "#[derive(glib::Enum)] struct Invalid; fn main() {}",
            "#[derive(glib::Enum)] only supports enums",
        ),
        (
            "enum_requires_name",
            "#[derive(glib::Enum)] enum Invalid { Value } fn main() {}",
            "#[derive(glib::Enum)] requires #[enum_type(name = \"EnumTypeName\")]",
        ),
        (
            "flags_requires_enum",
            "#[glib::flags(name = \"InvalidFlags\")] struct Invalid; fn main() {}",
            "#[glib::flags] only supports enums",
        ),
        (
            "boxed_requires_name",
            "#[derive(glib::Boxed)] struct Invalid; fn main() {}",
            "#[derive(glib::Boxed)] requires #[boxed_type(name = \"BoxedTypeName\")]",
        ),
        (
            "shared_boxed_requires_wrapper",
            "#[derive(glib::SharedBoxed)] struct Invalid; fn main() {}",
            "#[derive(glib::SharedBoxed)] requires struct MyStruct(T: RefCounted)",
        ),
        (
            "error_domain_requires_enum",
            "#[derive(glib::ErrorDomain)] struct Invalid; fn main() {}",
            "#[derive(glib::ErrorDomain)] only supports enums",
        ),
        (
            "object_subclass_requires_impl",
            "#[glib::object_subclass] struct Invalid; fn main() {}",
            "This macro should be used on `impl` block for `glib::ObjectSubclass` trait",
        ),
        (
            "object_interface_requires_trait",
            "struct Invalid; #[glib::object_interface] impl Invalid {} fn main() {}",
            "This macro should be used on `impl` block for `glib::ObjectInterface` trait",
        ),
        (
            "derived_properties_requires_trait",
            "struct Invalid; #[glib::derived_properties] impl Invalid {} fn main() {}",
            "This macro should be used on `impl` block for `glib::ObjectImpl` trait",
        ),
        (
            "variant_rejects_unknown_mode",
            "#[derive(glib::Variant)] #[variant_enum(unknown)] enum Invalid { Value } fn main() {}",
            "unknown type in #[variant_enum] attribute",
        ),
        (
            "closure_rejects_unknown_capture",
            "fn main() { let _ = glib::closure!(@unknown item => move || {}); }",
            "Unknown keyword `unknown`",
        ),
    ];

    for (name, source, message) in invalid {
        cases.compile_fail_inline_check_sub(name, source, message);
    }
}
