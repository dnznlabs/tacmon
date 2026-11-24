use ratatui::{Terminal, backend::TestBackend, text::{Line, Span}, widgets::{Block, Cell, Paragraph, Row, Table, Tabs}, layout::Constraint};
fn main() {
    let hostile = "A\rB\nC\tD\u{85}E\u{9b}F\u{1b}G\u{7}H\u{0}I";
    let mut t = Terminal::new(TestBackend::new(60, 8)).unwrap();
    t.draw(|f| {
        let a = f.area();
        let r = |y| ratatui::layout::Rect { x: 0, y, width: 60, height: 1 };
        f.render_widget(Paragraph::new(Line::from(Span::raw(hostile))), r(0));
        f.render_widget(Block::bordered().title(hostile), ratatui::layout::Rect { x: 0, y: 1, width: 60, height: 3 });
        f.render_widget(Tabs::new(vec![hostile]), r(4));
        f.render_widget(Table::new(vec![Row::new(vec![Cell::from(hostile)])], [Constraint::Fill(1)]), r(5));
        f.render_widget(Paragraph::new(hostile), ratatui::layout::Rect { x: 0, y: 6, width: 60, height: 2 });
        let _ = a;
    }).unwrap();
    for (i, c) in t.backend().buffer().content().iter().enumerate() {
        let s = c.symbol();
        if s.chars().any(|ch| ch.is_control()) { println!("control at cell {i} (row {}): {:?}", i / 60, s); }
    }
    let rows: Vec<String> = (0..8).map(|y| (0..60).map(|x| t.backend().buffer()[(x, y)].symbol().to_string()).collect()).collect();
    for r in rows { println!("{r:?}"); }
}
