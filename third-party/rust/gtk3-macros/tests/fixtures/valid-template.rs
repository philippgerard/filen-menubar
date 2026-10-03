use gtk::{glib, prelude::InitializingWidgetExt, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(file = "backport-widget.ui")]
    pub struct BackportWidget {
        #[template_child(id = "label")]
        pub label: TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BackportWidget {
        const NAME: &'static str = "BackportWidget";
        type Type = super::BackportWidget;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for BackportWidget {}
    impl WidgetImpl for BackportWidget {}
    impl ContainerImpl for BackportWidget {}
    impl BoxImpl for BackportWidget {}
}

glib::wrapper! {
    pub struct BackportWidget(ObjectSubclass<imp::BackportWidget>)
        @extends gtk::Widget, gtk::Container, gtk::Box;
}

fn main() {}
