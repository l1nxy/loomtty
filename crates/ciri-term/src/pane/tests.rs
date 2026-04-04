use super::*;

    fn shell_path() -> &'static str {
        if std::path::Path::new("/bin/sh").exists() {
            "/bin/sh"
        } else {
            "sh"
        }
    }

    fn new_test_pane() -> Pane {
        Pane::new(7, 4, 3, shell_path()).expect("create test pane")
    }

    #[test]
    fn set_cell_size_rounds_to_window_size_pixels() {
        let mut pane = new_test_pane();
        pane.set_cell_size(9.4, 17.6);

        let window_size = pane.window_size();
        assert_eq!(window_size.num_cols, 4);
        assert_eq!(window_size.num_lines, 3);
        assert_eq!(window_size.cell_width, 9);
        assert_eq!(window_size.cell_height, 18);
    }

    #[test]
    fn extract_damage_resets_after_read() {
        let mut pane = new_test_pane();

        let first = pane.extract_damage().expect("new pane should start dirty");
        assert_eq!(first, vec![(0, 0, 3), (1, 0, 3), (2, 0, 3)]);

        let second = pane
            .extract_damage()
            .expect("alacritty keeps the cursor line dirty");
        assert_eq!(second, vec![(0, 0, 3)]);

        let third = pane
            .extract_damage()
            .expect("cursor line damage remains stable after reset");
        assert_eq!(third, vec![(0, 0, 3)]);
    }

    #[test]
    fn snapshot_incremental_only_includes_new_scrollback() {
        let pane = new_test_pane();

        let none_sent = pane.snapshot_incremental(11, 0);
        let over_sent = pane.snapshot_incremental(12, 99);

        assert_eq!(none_sent.scrollback_rows, 0);
        assert!(none_sent.scrollback.is_empty());
        assert_eq!(over_sent.scrollback_rows, 0);
        assert!(over_sent.scrollback.is_empty());
    }

    #[test]
    fn drain_images_leaves_active_images_available_for_reconnect() {
        let mut pane = new_test_pane();
        let image = ImagePlacement {
            id: 1,
            row: 2,
            col: 3,
            width_cells: 4,
            height_cells: 5,
            pixel_width: 6,
            pixel_height: 7,
            display_mode: ImageDisplayMode::Cells,
            format: "png".into(),
            data: Arc::new(vec![1, 2, 3]),
        };

        pane.images.add_placements(vec![image.clone()]);
        pane.images.active_mut().push(image.clone());

        let drained = pane.drain_images();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].id, image.id);
        assert_eq!(pane.active_images().len(), 1);
        assert_eq!(pane.active_images()[0].id, image.id);
        assert!(pane.drain_images().is_empty());
    }

    #[test]
    fn delete_state_drains_once_after_clear() {
        let mut pane = new_test_pane();

        pane.images.clear_on_delete();

        assert!(pane.drain_image_deletes());
        assert!(!pane.drain_image_deletes());
    }
