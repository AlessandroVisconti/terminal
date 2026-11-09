//! The layers inside the macOS blur view (`NSVisualEffectView`): a description of the tree the
//! view builds, the debug-only dump `FBTERM_DEMO_BLUR_DUMP=1`, and (classic style) which
//! layers to hide so only the blurred backdrop stays, without the material's own fill.

/// One layer of a blur view's tree, as far as choosing what to hide needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerNode {
    /// The layer's class name (for the dump; never the reason to hide or keep a layer).
    pub class: String,
    /// The names of its filters (`CALayer.filters`).
    pub filters: Vec<String>,
    pub children: Vec<LayerNode>,
}

impl LayerNode {
    /// The tree in one line, classes nested in brackets with their filters, for the log line
    /// that says the tree was not as expected: `A[B(gaussianBlur) C]`.
    pub fn summary(&self) -> String {
        let mut out = self.class.clone();
        if !self.filters.is_empty() {
            out.push_str(&format!("({})", self.filters.join(",")));
        }
        if !self.children.is_empty() {
            let children: Vec<String> = self.children.iter().map(Self::summary).collect();
            out.push_str(&format!("[{}]", children.join(" ")));
        }
        out
    }
}

/// Whether a filter name is a blur: the backdrop layer carries the Gaussian blur.
pub fn is_blur_filter(name: &str) -> bool {
    name.to_ascii_lowercase().contains("blur")
}

/// The layers to hide for a fill-free blur, as child-index paths from `root` (the blur view's
/// own layer, never hidden): every layer that is neither the one blurring layer, nor one of
/// its ancestors, nor inside it. `None` when the tree does not have the expected shape (no
/// blurring layer, or more than one, or the root itself blurs): the caller then leaves the
/// view as the system draws it.
pub fn layers_to_hide(root: &LayerNode) -> Option<Vec<Vec<usize>>> {
    fn blurring(node: &LayerNode, path: &mut Vec<usize>, found: &mut Vec<Vec<usize>>) {
        if node.filters.iter().any(|f| is_blur_filter(f)) {
            found.push(path.clone());
            // A blur inside the blurring layer is part of it.
            return;
        }
        for (i, child) in node.children.iter().enumerate() {
            path.push(i);
            blurring(child, path, found);
            path.pop();
        }
    }
    let mut found = Vec::new();
    blurring(root, &mut Vec::new(), &mut found);
    let [keep] = found.as_slice() else {
        return None;
    };
    if keep.is_empty() {
        return None;
    }
    // Walk down to the blurring layer; at each level hide the siblings off the path.
    let mut hide = Vec::new();
    let mut node = root;
    for (depth, &step) in keep.iter().enumerate() {
        for i in (0..node.children.len()).filter(|&i| i != step) {
            let mut path = keep[..depth].to_vec();
            path.push(i);
            hide.push(path);
        }
        node = &node.children[step];
    }
    Some(hide)
}

/// What to do with a classic blur view's layers.
#[derive(Debug, Clone, PartialEq)]
pub enum ClearPlan {
    /// The view has no layers yet (not in a window, not drawn): try again once it has.
    NotBuilt,
    /// Built, but not the expected shape: leave it as the system draws it.
    Unexpected,
    /// Hide these layers ([`layers_to_hide`]).
    Hide(Vec<Vec<usize>>),
}

/// The plan for a view whose layer tree is `root` (`None`: no layer at all).
pub fn plan_clear(root: Option<&LayerNode>) -> ClearPlan {
    match root {
        None => ClearPlan::NotBuilt,
        Some(root) if root.children.is_empty() => ClearPlan::NotBuilt,
        Some(root) => layers_to_hide(root).map_or(ClearPlan::Unexpected, ClearPlan::Hide),
    }
}

/// What a classic blur view does with a plan.
#[derive(Debug, Clone, PartialEq)]
pub enum ClearStep {
    /// Leave the layers as they are: not built yet, or the view already fell back.
    Nothing,
    /// Hide these layers.
    Hide(Vec<Vec<usize>>),
    /// Switch to the fallback material, say so once, and leave its layers as drawn.
    FallBack,
}

/// The step for `plan` in a view that has (`fell_back`) or has not fallen back already.
pub fn clear_step(fell_back: bool, plan: ClearPlan) -> ClearStep {
    match plan {
        _ if fell_back => ClearStep::Nothing,
        ClearPlan::NotBuilt => ClearStep::Nothing,
        ClearPlan::Unexpected => ClearStep::FallBack,
        ClearPlan::Hide(paths) => ClearStep::Hide(paths),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(class: &str, filters: &[&str], children: Vec<LayerNode>) -> LayerNode {
        LayerNode {
            class: class.into(),
            filters: filters.iter().map(|f| (*f).to_owned()).collect(),
            children,
        }
    }

    /// The tree macOS 27 builds for a blurring material (measured with
    /// `FBTERM_DEMO_BLUR_DUMP=1`): the view's layer, the material's layer, and under it the
    /// backdrop, the fill, the tone and the tint.
    fn measured(backdrop_filters: &[&str]) -> LayerNode {
        layer(
            "NSViewBackingLayer",
            &[],
            vec![layer(
                "CALayer",
                &[],
                vec![
                    layer("CABackdropLayer", backdrop_filters, vec![]),
                    layer("CALayer", &[], vec![]),
                    layer("CALayer", &[], vec![]),
                    layer("CAChameleonLayer", &[], vec![]),
                ],
            )],
        )
    }

    #[test]
    fn everything_but_the_blurring_layer_is_hidden() {
        for filters in [
            &["sdrNormalize", "gaussianBlur", "colorSaturate"][..],
            &["sdrNormalize", "gaussianBlur"],
            &["sdrNormalize", "gaussianBlur 2", "colorSaturate"],
            &["sdrNormalize", "blur", "saturate"],
        ] {
            assert_eq!(
                layers_to_hide(&measured(filters)),
                Some(vec![vec![0, 1], vec![0, 2], vec![0, 3]]),
                "{filters:?}"
            );
        }
    }

    #[test]
    fn the_blurring_layer_is_found_by_its_filter_not_its_class() {
        // Renamed classes, backdrop not first, siblings at two levels.
        let tree = layer(
            "Root",
            &[],
            vec![
                layer("Other", &[], vec![layer("Inner", &[], vec![])]),
                layer(
                    "Group",
                    &[],
                    vec![
                        layer("Fill", &[], vec![]),
                        layer(
                            "Renamed",
                            &["gaussianBlur"],
                            vec![layer("Kept", &[], vec![])],
                        ),
                    ],
                ),
            ],
        );
        assert_eq!(layers_to_hide(&tree), Some(vec![vec![0], vec![1, 0]]));
        // A lone backdrop: nothing to hide.
        let tree = layer("Root", &[], vec![layer("B", &["gaussianBlur"], vec![])]);
        assert_eq!(layers_to_hide(&tree), Some(vec![]));
        assert!(is_blur_filter("gaussianBlur") && is_blur_filter("blur"));
        assert!(!is_blur_filter("colorSaturate") && !is_blur_filter("sdrNormalize"));
    }

    #[test]
    fn an_unexpected_tree_is_left_alone() {
        // The opaque materials: a hosting layer with plain color layers, no blur at all.
        let opaque = layer(
            "NSViewBackingLayer",
            &[],
            vec![layer(
                "CAHostingLayer",
                &[],
                vec![
                    layer("CALayer", &[], vec![]),
                    layer("CAChameleonLayer", &[], vec![]),
                ],
            )],
        );
        assert_eq!(layers_to_hide(&opaque), None);
        // Two blurring layers: which one is the backdrop is not clear.
        let two = layer(
            "Root",
            &[],
            vec![
                layer("A", &["gaussianBlur"], vec![]),
                layer("B", &["gaussianBlur"], vec![]),
            ],
        );
        assert_eq!(layers_to_hide(&two), None);
        // The view's own layer blurring: nothing could be hidden around it.
        assert_eq!(layers_to_hide(&layer("Root", &["blur"], vec![])), None);
        // No layers at all.
        assert_eq!(layers_to_hide(&layer("Root", &[], vec![])), None);
        // A view whose layers are not built yet is not unexpected: it is cleared once built.
        assert_eq!(plan_clear(None), ClearPlan::NotBuilt);
        assert_eq!(
            plan_clear(Some(&layer("Root", &[], vec![]))),
            ClearPlan::NotBuilt
        );
        assert_eq!(plan_clear(Some(&opaque)), ClearPlan::Unexpected);
        assert_eq!(plan_clear(Some(&two)), ClearPlan::Unexpected);
        assert_eq!(
            plan_clear(Some(&measured(&["gaussianBlur"]))),
            ClearPlan::Hide(vec![vec![0, 1], vec![0, 2], vec![0, 3]])
        );
        // What the view does with a plan: hide, fall back once, or wait. After a fallback it
        // leaves its layers alone, whatever later hooks find.
        let hide = ClearPlan::Hide(vec![vec![0, 1]]);
        assert_eq!(
            clear_step(false, hide.clone()),
            ClearStep::Hide(vec![vec![0, 1]])
        );
        assert_eq!(
            clear_step(false, ClearPlan::Unexpected),
            ClearStep::FallBack
        );
        assert_eq!(clear_step(false, ClearPlan::NotBuilt), ClearStep::Nothing);
        for plan in [hide, ClearPlan::Unexpected, ClearPlan::NotBuilt] {
            assert_eq!(clear_step(true, plan), ClearStep::Nothing);
        }
        // What the log line shows of such a tree.
        assert_eq!(two.summary(), "Root[A(gaussianBlur) B(gaussianBlur)]");
    }
}

#[cfg(target_os = "macos")]
pub(crate) mod mac {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_app_kit::NSVisualEffectView;
    use objc2_foundation::{NSArray, NSString};

    use super::{ClearPlan, ClearStep, LayerNode};

    fn text(object: Option<&AnyObject>) -> String {
        object.map_or_else(
            || "nil".to_owned(),
            |o| {
                // SAFETY: `description` is NSObject's, answered by every object here.
                let d: Retained<NSString> = unsafe { msg_send![o, description] };
                d.to_string().replace('\n', " ")
            },
        )
    }

    /// The name of a filter object: `name` when it answers it (Core Animation filters do),
    /// else its description.
    fn filter_name(filter: &AnyObject) -> String {
        // SAFETY: plain NSObject messages on a live object.
        unsafe {
            let responds: bool = msg_send![filter, respondsToSelector: objc2::sel!(name)];
            if responds {
                let name: Option<Retained<AnyObject>> = msg_send![filter, name];
                text(name.as_deref())
            } else {
                text(Some(filter))
            }
        }
    }

    pub(crate) fn sublayers(layer: &AnyObject) -> Vec<Retained<AnyObject>> {
        // SAFETY: `sublayers` is CALayer's, returning an optional NSArray of layers.
        let subs: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![layer, sublayers] };
        subs.map(|a| a.to_vec()).unwrap_or_default()
    }

    fn filters(layer: &AnyObject) -> Vec<String> {
        // SAFETY: `filters` is CALayer's, an optional NSArray.
        let f: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![layer, filters] };
        f.map(|a| a.iter().map(|x| filter_name(&x)).collect())
            .unwrap_or_default()
    }

    /// The view's own layer, if it is layer-backed.
    pub(crate) fn root_layer(view: &NSVisualEffectView) -> Option<Retained<AnyObject>> {
        // SAFETY: `layer` is NSView's, an optional CALayer; main thread (callers hold a view).
        unsafe { msg_send![view, layer] }
    }

    pub(crate) fn describe(layer: &AnyObject) -> LayerNode {
        LayerNode {
            class: layer.class().name().to_string_lossy().into_owned(),
            filters: filters(layer),
            children: sublayers(layer).iter().map(|l| describe(l)).collect(),
        }
    }

    /// One line per layer, indented by depth: class, name, hidden, opacity, background
    /// color, filters, compositing filter.
    pub(crate) fn dump(layer: &AnyObject, depth: usize, out: &mut String) {
        type Object = Option<Retained<AnyObject>>;
        let key = NSString::from_str("backgroundColor");
        // SAFETY: CALayer property getters on a live layer, main thread; the background color
        // is read through key-value coding, which boxes the CGColor as an object.
        let hidden: bool = unsafe { msg_send![layer, isHidden] };
        let opacity: f32 = unsafe { msg_send![layer, opacity] };
        let name: Object = unsafe { msg_send![layer, name] };
        let background: Object = unsafe { msg_send![layer, valueForKey: &*key] };
        let compositing: Object = unsafe { msg_send![layer, compositingFilter] };
        out.push_str(&format!(
            "{:indent$}{} name={} hidden={hidden} opacity={opacity} bg={} filters={:?} compositing={}\n",
            "",
            layer.class().name().to_string_lossy(),
            text(name.as_deref()),
            text(background.as_deref()),
            filters(layer),
            text(compositing.as_deref()),
            indent = depth * 2,
        ));
        for sub in sublayers(layer) {
            dump(&sub, depth + 1, out);
        }
    }

    /// Commit pending Core Animation changes, so the tree a material change builds is there.
    pub(crate) fn flush() {
        if let Some(class) = AnyClass::get(c"CATransaction") {
            // SAFETY: `+[CATransaction flush]` takes no arguments; main thread.
            let () = unsafe { msg_send![class, flush] };
        }
    }

    /// The plan for `view`'s layers now (see [`super::plan_clear`]).
    fn plan(view: &NSVisualEffectView) -> (Option<Retained<AnyObject>>, ClearPlan) {
        let root = root_layer(view);
        let plan = super::plan_clear(root.as_deref().map(describe).as_ref());
        (root, plan)
    }

    /// Hide the layers at `paths` under `root`, in one Core Animation transaction with
    /// implicit actions off, so a re-hide never fades the fill in and out. False if a path
    /// no longer leads to a layer.
    fn hide(root: &Retained<AnyObject>, paths: &[Vec<usize>]) -> bool {
        let class = AnyClass::get(c"CATransaction");
        if let Some(class) = class {
            // SAFETY: `+[CATransaction begin]` and `+setDisableActions:` take what is passed;
            // main thread. Every `begin` here is matched by the `commit` below.
            unsafe {
                let () = msg_send![class, begin];
                let () = msg_send![class, setDisableActions: true];
            }
        }
        let mut whole = true;
        'paths: for path in paths {
            let mut layer = root.clone();
            for &i in path {
                match sublayers(&layer).into_iter().nth(i) {
                    Some(next) => layer = next,
                    None => {
                        whole = false;
                        continue 'paths;
                    }
                }
            }
            // SAFETY: `setHidden:` is CALayer's; main thread.
            let () = unsafe { msg_send![&*layer, setHidden: true] };
        }
        if let Some(class) = class {
            // SAFETY: matches the `begin` above.
            let () = unsafe { msg_send![class, commit] };
        }
        whole
    }

    /// Log once per run that the classic blur fell back, naming the layers found.
    fn log_fallback(root: Option<&AnyObject>) {
        static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let layers = root.map(|r| describe(r).summary()).unwrap_or_default();
            tracing::info!(
                layers = %layers,
                "the classic blur cannot hide this system's blur fill; using the lightest material"
            );
        }
    }

    objc2::define_class!(
        // SAFETY: NSVisualEffectView may be subclassed; the overrides only call `super` and
        // then hide layers; the one ivar is a plain `Cell<bool>`, and there is no `Drop`.
        #[unsafe(super(NSVisualEffectView, NSView, NSResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FbtermClearBlurView"]
        #[ivars = Cell<bool>]
        /// The classic style's blur view: the system's blur view that hides its own fill
        /// again after every change that rebuilds or restyles its layers. Its ivar is set once
        /// it fell back to the fallback material, which it then leaves as drawn.
        pub(crate) struct ClearBlurView;

        impl ClearBlurView {
            #[unsafe(method(layout))]
            fn layout(&self) {
                // SAFETY: the superclass implements `layout`.
                let () = unsafe { msg_send![super(self), layout] };
                self.clear();
            }

            #[unsafe(method(updateLayer))]
            fn update_layer(&self) {
                // SAFETY: NSView implements `updateLayer`.
                let () = unsafe { msg_send![super(self), updateLayer] };
                self.clear();
            }

            #[unsafe(method(viewDidChangeEffectiveAppearance))]
            fn appearance_changed(&self) {
                // SAFETY: NSView implements `viewDidChangeEffectiveAppearance`.
                let () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
                self.clear();
            }

            #[unsafe(method(viewDidChangeBackingProperties))]
            fn backing_changed(&self) {
                // SAFETY: NSView implements `viewDidChangeBackingProperties`.
                let () = unsafe { msg_send![super(self), viewDidChangeBackingProperties] };
                self.clear();
            }

            #[unsafe(method(viewDidMoveToWindow))]
            fn moved_to_window(&self) {
                // SAFETY: NSView implements `viewDidMoveToWindow`.
                let () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
                self.clear();
            }
        }
    );

    impl ClearBlurView {
        /// A new classic blur view of `frame`.
        pub(crate) fn create(
            mtm: objc2::MainThreadMarker,
            frame: objc2_foundation::NSRect,
        ) -> Retained<Self> {
            let this = mtm.alloc::<Self>().set_ivars(Cell::new(false));
            // SAFETY: `initWithFrame:` is NSView's designated initializer.
            unsafe { msg_send![super(this), initWithFrame: frame] }
        }

        /// Hide the fill, or fall back once (fallback material, one log line), or wait for
        /// the layers to be built (see [`super::clear_step`]). Every hook and every tune runs
        /// this one path.
        pub(crate) fn clear(&self) {
            if self.ivars().get() {
                return;
            }
            let (root, plan) = plan(self);
            match super::clear_step(false, plan) {
                ClearStep::Nothing => {}
                ClearStep::Hide(paths) => {
                    if let Some(root) = &root
                        && !hide(root, &paths)
                    {
                        self.fall_back(Some(&**root));
                    }
                }
                ClearStep::FallBack => self.fall_back(root.as_deref()),
            }
        }

        /// A new look was just set (material, appearance): try hiding its fill afresh, even
        /// after an earlier fallback.
        pub(crate) fn retry(&self) {
            self.ivars().set(false);
            self.clear();
        }

        fn fall_back(&self, root: Option<&AnyObject>) {
            use objc2_app_kit::NSVisualEffectMaterial;
            // Set first: the material change below lays the view out again, whose hook then
            // returns at once.
            self.ivars().set(true);
            log_fallback(root);
            let fallback = NSVisualEffectMaterial(crate::chrome::BlurLook::CLASSIC_FALLBACK.0);
            if self.material() != fallback {
                self.setMaterial(fallback);
            }
        }
    }

    use std::cell::Cell;

    use objc2::runtime::NSObject;
    use objc2::{DefinedClass, MainThreadOnly};
    use objc2_app_kit::{NSResponder, NSView};
}
