package primitive

import "fmt"

// EncodeShape flattens a shape into numbers: its ShapeType followed by its
// fields. DecodeShape turns them back into a shape owned by worker. The
// browser version uses this to pass shapes between models running in
// separate Web Workers; the encoding is exact, so the copies stay identical.
func EncodeShape(s Shape) []float64 {
	switch s := s.(type) {
	case *Triangle:
		return []float64{float64(ShapeTypeTriangle),
			float64(s.X1), float64(s.Y1), float64(s.X2), float64(s.Y2), float64(s.X3), float64(s.Y3)}
	case *Rectangle:
		return []float64{float64(ShapeTypeRectangle),
			float64(s.X1), float64(s.Y1), float64(s.X2), float64(s.Y2)}
	case *Ellipse:
		t := ShapeTypeEllipse
		if s.Circle {
			t = ShapeTypeCircle
		}
		return []float64{float64(t), float64(s.X), float64(s.Y), float64(s.Rx), float64(s.Ry)}
	case *RotatedRectangle:
		return []float64{float64(ShapeTypeRotatedRectangle),
			float64(s.X), float64(s.Y), float64(s.Sx), float64(s.Sy), float64(s.Angle)}
	case *Quadratic:
		return []float64{float64(ShapeTypeQuadratic), s.X1, s.Y1, s.X2, s.Y2, s.X3, s.Y3, s.Width}
	case *RotatedEllipse:
		return []float64{float64(ShapeTypeRotatedEllipse), s.X, s.Y, s.Rx, s.Ry, s.Angle}
	case *Polygon:
		v := []float64{float64(ShapeTypePolygon), 0}
		if s.Convex {
			v[1] = 1
		}
		for i := 0; i < s.Order; i++ {
			v = append(v, s.X[i], s.Y[i])
		}
		return v
	}
	panic(fmt.Sprintf("primitive: cannot encode %T", s))
}

func DecodeShape(worker *Worker, v []float64) (Shape, error) {
	if len(v) == 0 {
		return nil, fmt.Errorf("primitive: empty shape")
	}
	n := func(want int) error {
		if len(v) != want {
			return fmt.Errorf("primitive: shape type %d needs %d numbers, got %d", int(v[0]), want, len(v))
		}
		return nil
	}
	i := func(k int) int { return int(v[k]) }
	switch ShapeType(v[0]) {
	case ShapeTypeTriangle:
		if err := n(7); err != nil {
			return nil, err
		}
		return &Triangle{worker, i(1), i(2), i(3), i(4), i(5), i(6)}, nil
	case ShapeTypeRectangle:
		if err := n(5); err != nil {
			return nil, err
		}
		return &Rectangle{worker, i(1), i(2), i(3), i(4)}, nil
	case ShapeTypeEllipse, ShapeTypeCircle:
		if err := n(5); err != nil {
			return nil, err
		}
		return &Ellipse{worker, i(1), i(2), i(3), i(4), ShapeType(v[0]) == ShapeTypeCircle}, nil
	case ShapeTypeRotatedRectangle:
		if err := n(6); err != nil {
			return nil, err
		}
		return &RotatedRectangle{worker, i(1), i(2), i(3), i(4), i(5)}, nil
	case ShapeTypeQuadratic:
		if err := n(8); err != nil {
			return nil, err
		}
		return &Quadratic{worker, v[1], v[2], v[3], v[4], v[5], v[6], v[7]}, nil
	case ShapeTypeRotatedEllipse:
		if err := n(6); err != nil {
			return nil, err
		}
		return &RotatedEllipse{worker, v[1], v[2], v[3], v[4], v[5]}, nil
	case ShapeTypePolygon:
		if len(v) < 8 || len(v)%2 != 0 {
			return nil, fmt.Errorf("primitive: bad polygon of %d numbers", len(v))
		}
		p := &Polygon{Worker: worker, Order: (len(v) - 2) / 2, Convex: v[1] != 0}
		for k := 2; k < len(v); k += 2 {
			p.X = append(p.X, v[k])
			p.Y = append(p.Y, v[k+1])
		}
		return p, nil
	}
	return nil, fmt.Errorf("primitive: unknown shape type %d", int(v[0]))
}
