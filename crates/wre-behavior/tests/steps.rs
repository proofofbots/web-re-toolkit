use wre_behavior::stream::{Point, Shape, Stream};

#[test]
fn the_step_size_decides_how_many_moves_a_flight_makes() {
    let dense = {
        let mut stream = Stream::new(7, Point::new(180.0, 260.0), Shape::default());
        let _ = stream.move_to(Point::new(412.0, 308.0));
        stream.events().len()
    };

    let coarse = {
        let mut stream = Stream::new(
            7,
            Point::new(180.0, 260.0),
            Shape { step_px: 26.0, ..Shape::default() },
        );
        let _ = stream.move_to(Point::new(412.0, 308.0));
        stream.events().len()
    };

    assert!(coarse * 3 < dense, "coarse {coarse} is not far below dense {dense}");
}
