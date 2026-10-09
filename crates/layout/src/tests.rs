use super::*;
use figcraft_doc::{AutoLayout, Document, NodeKind};

fn setup(mode: LayoutMode) -> (Document, NodeId, Vec<NodeId>) {
    let mut d = Document::new("t");
    let p = d.first_page().unwrap();
    let f = d.create(p, NodeKind::Frame, "F", 0.0, 0.0, 300.0, 100.0).unwrap();
    d.get_mut(f).unwrap().layout = Some(AutoLayout { mode, gap: 10.0, padding: [5.0, 5.0, 5.0, 5.0], ..Default::default() });
    let a = d.create(f, NodeKind::Rectangle, "A", 0.0, 0.0, 50.0, 20.0).unwrap();
    let b = d.create(f, NodeKind::Rectangle, "B", 0.0, 0.0, 30.0, 40.0).unwrap();
    (d, f, vec![a, b])
}

fn pos(d: &Document, id: NodeId) -> (f64, f64, f64, f64) {
    let n = d.node(id).unwrap();
    let c = n.transform.as_coeffs();
    (c[4], c[5], n.width, n.height)
}

#[test]
fn horizontal_stack() {
    let (mut d, f, k) = setup(LayoutMode::Horizontal);
    layout_frame(&mut d, f, &NoMeasure);
    assert_eq!(pos(&d, k[0]), (5.0, 5.0, 50.0, 20.0));
    assert_eq!(pos(&d, k[1]), (65.0, 5.0, 30.0, 40.0));
}

#[test]
fn hug_container() {
    let (mut d, f, _) = setup(LayoutMode::Horizontal);
    let n = d.get_mut(f).unwrap();
    n.child_layout.sizing_h = Sizing::Hug;
    n.child_layout.sizing_v = Sizing::Hug;
    layout_frame(&mut d, f, &NoMeasure);
    let n = d.node(f).unwrap();
    assert_eq!((n.width, n.height), (5.0 + 50.0 + 10.0 + 30.0 + 5.0, 50.0));
}

#[test]
fn vertical_center_alignment() {
    let (mut d, f, k) = setup(LayoutMode::Vertical);
    d.get_mut(f).unwrap().layout.as_mut().unwrap().cross_align = CrossAlign::Center;
    layout_frame(&mut d, f, &NoMeasure);
    // Inner width 290; A (50 wide) centred → x = 5 + 120.
    assert_eq!(pos(&d, k[0]).0, 125.0);
    assert_eq!(pos(&d, k[1]).1, 5.0 + 20.0 + 10.0);
}

#[test]
fn fill_children_share_space() {
    let (mut d, f, k) = setup(LayoutMode::Horizontal);
    for c in &k {
        d.get_mut(*c).unwrap().child_layout.sizing_h = Sizing::Fill;
    }
    d.get_mut(k[1]).unwrap().child_layout.sizing_v = Sizing::Fill;
    layout_frame(&mut d, f, &NoMeasure);
    // Inner 290 - gap 10 = 280 → 140 each.
    assert_eq!(pos(&d, k[0]).2, 140.0);
    assert_eq!(pos(&d, k[1]).0, 5.0 + 140.0 + 10.0);
    assert_eq!(pos(&d, k[1]).3, 90.0);
    // Max width on one.
    d.get_mut(k[0]).unwrap().child_layout.max_w = Some(100.0);
    layout_frame(&mut d, f, &NoMeasure);
    assert_eq!(pos(&d, k[0]).2, 100.0);
    assert_eq!(pos(&d, k[1]).2, 180.0);
}

#[test]
fn space_between_and_absolute() {
    let (mut d, f, k) = setup(LayoutMode::Horizontal);
    d.get_mut(f).unwrap().layout.as_mut().unwrap().main_align = MainAlign::SpaceBetween;
    let abs = d.create(f, NodeKind::Rectangle, "Abs", 7.0, 7.0, 10.0, 10.0).unwrap();
    d.get_mut(abs).unwrap().child_layout.absolute = true;
    layout_frame(&mut d, f, &NoMeasure);
    assert_eq!(pos(&d, k[1]).0, 300.0 - 5.0 - 30.0);
    assert_eq!(pos(&d, abs).0, 7.0);
}

#[test]
fn wrap_into_rows() {
    let mut d = Document::new("t");
    let p = d.first_page().unwrap();
    let f = d.create(p, NodeKind::Frame, "F", 0.0, 0.0, 100.0, 100.0).unwrap();
    d.get_mut(f).unwrap().layout = Some(AutoLayout { wrap: true, gap: 10.0, cross_gap: 5.0, padding: [0.0; 4], ..Default::default() });
    d.get_mut(f).unwrap().child_layout.sizing_v = Sizing::Hug;
    let ids: Vec<NodeId> = (0..5).map(|i| d.create(f, NodeKind::Rectangle, &format!("R{i}"), 0.0, 0.0, 40.0, 20.0).unwrap()).collect();
    layout_frame(&mut d, f, &NoMeasure);
    // Two per row (40+10+40 = 90 ≤ 100).
    assert_eq!(pos(&d, ids[2]), (0.0, 25.0, 40.0, 20.0));
    assert_eq!(d.node(f).unwrap().height, 20.0 * 3.0 + 5.0 * 2.0);
}

#[test]
fn grid_mode() {
    let mut d = Document::new("t");
    let p = d.first_page().unwrap();
    let f = d.create(p, NodeKind::Frame, "F", 0.0, 0.0, 210.0, 110.0).unwrap();
    d.get_mut(f).unwrap().layout = Some(AutoLayout {
        mode: LayoutMode::Grid,
        grid_columns: 2,
        grid_rows: 2,
        gap: 10.0,
        cross_gap: 10.0,
        padding: [0.0; 4],
        ..Default::default()
    });
    let ids: Vec<NodeId> = (0..4).map(|i| d.create(f, NodeKind::Rectangle, &format!("R{i}"), 0.0, 0.0, 10.0, 10.0).unwrap()).collect();
    layout_frame(&mut d, f, &NoMeasure);
    assert_eq!(pos(&d, ids[3]).0, 110.0);
    assert_eq!(pos(&d, ids[3]).1, 60.0);
}

#[test]
fn nested_hug_propagates_with_layout_page() {
    let mut d = Document::new("t");
    let p = d.first_page().unwrap();
    let outer = d.create(p, NodeKind::Frame, "Outer", 0.0, 0.0, 10.0, 10.0).unwrap();
    d.get_mut(outer).unwrap().layout = Some(AutoLayout { padding: [0.0; 4], ..Default::default() });
    d.get_mut(outer).unwrap().child_layout.sizing_h = Sizing::Hug;
    let inner = d.create(outer, NodeKind::Frame, "Inner", 0.0, 0.0, 10.0, 10.0).unwrap();
    d.get_mut(inner).unwrap().layout = Some(AutoLayout { padding: [0.0; 4], gap: 0.0, ..Default::default() });
    d.get_mut(inner).unwrap().child_layout.sizing_h = Sizing::Hug;
    d.create(inner, NodeKind::Rectangle, "R", 0.0, 0.0, 77.0, 10.0).unwrap();
    layout_page(&mut d, p, &NoMeasure);
    assert_eq!(d.node(inner).unwrap().width, 77.0);
    assert_eq!(d.node(outer).unwrap().width, 77.0);
}

#[test]
fn constraints_on_resize() {
    let mut d = Document::new("t");
    let p = d.first_page().unwrap();
    let f = d.create(p, NodeKind::Frame, "F", 0.0, 0.0, 100.0, 100.0).unwrap();
    let r = d.create(f, NodeKind::Rectangle, "R", 80.0, 10.0, 10.0, 10.0).unwrap();
    d.get_mut(r).unwrap().constraints = figcraft_doc::Constraints { horizontal: HConstraint::Right, vertical: VConstraint::TopBottom };
    let c = d.create(f, NodeKind::Rectangle, "C", 45.0, 45.0, 10.0, 10.0).unwrap();
    d.get_mut(c).unwrap().constraints = figcraft_doc::Constraints { horizontal: HConstraint::Center, vertical: VConstraint::Scale };
    set_size(&mut d, f, 200.0, 200.0);
    apply_constraints(&mut d, f, 100.0, 100.0);
    assert_eq!(pos(&d, r), (180.0, 10.0, 10.0, 110.0));
    assert_eq!(pos(&d, c), (95.0, 90.0, 10.0, 20.0));
}

#[test]
fn grid_lines_columns_stretch() {
    let g = LayoutGrid { kind: GridKind::Columns, count: 4, gutter: 20.0, offset: 10.0, ..LayoutGrid::default() };
    let r = grid_lines(&g, 400.0, 100.0);
    assert_eq!(r.len(), 4);
    assert_eq!(r[0].x0, 10.0);
    assert!((r[3].x1 - 390.0).abs() < 1e-9);
}

#[test]
fn hostile_values() {
    let (mut d, f, _) = setup(LayoutMode::Horizontal);
    let n = d.get_mut(f).unwrap();
    n.layout.as_mut().unwrap().gap = f64::NAN;
    n.layout.as_mut().unwrap().padding = [f64::INFINITY, -1.0, f64::NAN, 1e300];
    n.layout.as_mut().unwrap().grid_columns = 0;
    layout_frame(&mut d, f, &NoMeasure);
    d.get_mut(f).unwrap().layout.as_mut().unwrap().mode = LayoutMode::Grid;
    layout_frame(&mut d, f, &NoMeasure);
    let _ = grid_lines(&LayoutGrid { size: 0.0, ..Default::default() }, 1e9, 1e9);
}
