// Exercise actual macro expansion without initializing GTK or a display.

#[test]
fn invalid_templates_report_compiler_errors() {
    let cases = trybuild2::TestCases::new();
    let invalid = [
        (
            "missing_template",
            "#[derive(gtk::CompositeTemplate)] struct Invalid; fn main() {}",
            "Missing 'template' attribute: derive(CompositeTemplate) requires #[template(...)]",
        ),
        (
            "malformed_template",
            "#[derive(gtk::CompositeTemplate)] #[template(unknown = \"\")] struct Invalid; fn main() {}",
            "derive(CompositeTemplate) requires #[template(...)]",
        ),
        (
            "template_requires_struct",
            "#[derive(gtk::CompositeTemplate)] #[template(string = \"\")] enum Invalid { Value } fn main() {}",
            "derive(CompositeTemplate) only supports structs",
        ),
        (
            "duplicate_template_child_id",
            "#[derive(gtk::CompositeTemplate)] #[template(string = \"\")] struct Invalid { #[template_child(id = \"a\", id = \"b\")] child: gtk::subclass::widget::TemplateChild<gtk::Label> } fn main() {}",
            "two instances of the same attribute argument, each argument must be specified only once",
        ),
    ];

    for (name, source, message) in invalid {
        cases.compile_fail_inline_check_sub(name, source, message);
    }
}

#[test]
fn valid_template_and_child_still_compile() {
    let cases = trybuild2::TestCases::new();
    cases.pass("tests/fixtures/valid-template.rs");
}
