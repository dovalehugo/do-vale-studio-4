//! Integration tests for the editor domain (8B.3).

use crate::*;

fn pid(n: u64) -> ProjectId {
    ProjectId::new(n).expect("project id")
}
fn aid(n: u64) -> MediaAssetId {
    MediaAssetId::new(n).expect("asset id")
}
fn tid(n: u64) -> TrackId {
    TrackId::new(n).expect("track id")
}
fn cid(n: u64) -> ClipId {
    ClipId::new(n).expect("clip id")
}
fn pos(us: u64) -> TimelinePosition {
    TimelinePosition::from_micros(us)
}
fn dur(us: u64) -> TimelineDuration {
    TimelineDuration::from_micros(us).expect("duration")
}
fn off(us: u64) -> SourceOffset {
    SourceOffset::from_micros(us)
}

fn editor() -> Editor {
    Editor::new(Project::new(pid(1), "Test").expect("project"))
}

fn register_asset(editor: &mut Editor, id: MediaAssetId) {
    editor
        .apply(EditCommand::RegisterAsset { id })
        .expect("register");
}

fn add_video(editor: &mut Editor, id: TrackId, name: &str) {
    editor
        .apply(EditCommand::AddTrack {
            id,
            name: name.into(),
            kind: TrackKind::Video,
        })
        .expect("add track");
}

fn insert_clip(
    editor: &mut Editor,
    id: ClipId,
    track_id: TrackId,
    asset_id: MediaAssetId,
    start: u64,
    duration: u64,
) -> Result<(), EditorError> {
    editor.apply(EditCommand::InsertClip {
        id,
        track_id,
        asset_id,
        start: pos(start),
        source_offset: off(0),
        duration: TimelineDuration::from_micros(duration)?,
    })
}

#[test]
fn media_pool_register_duplicate_remove_and_order() {
    let mut ed = editor();
    register_asset(&mut ed, aid(3));
    register_asset(&mut ed, aid(1));
    register_asset(&mut ed, aid(2));
    assert_eq!(
        ed.project().media_pool().ids().collect::<Vec<_>>(),
        vec![aid(1), aid(2), aid(3)]
    );
    assert_eq!(
        ed.apply(EditCommand::RegisterAsset { id: aid(2) }),
        Err(EditorError::DuplicateId)
    );
    ed.apply(EditCommand::UnregisterAsset { id: aid(2) })
        .expect("unregister");
    assert!(!ed.project().media_pool().contains(aid(2)));
}

#[test]
fn media_pool_rejects_removing_asset_in_use() {
    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 10).expect("insert");
    assert_eq!(
        ed.apply(EditCommand::UnregisterAsset { id: aid(1) }),
        Err(EditorError::AssetInUse)
    );
    assert!(ed.project().media_pool().contains(aid(1)));
    assert_eq!(ed.history().undo_len(), 3);
}

#[test]
fn tracks_add_video_audio_duplicate_empty_name_order_and_remove() {
    let mut ed = editor();
    add_video(&mut ed, tid(1), "V1");
    ed.apply(EditCommand::AddTrack {
        id: tid(2),
        name: "A1".into(),
        kind: TrackKind::Audio,
    })
    .expect("audio");
    assert_eq!(ed.project().timeline().tracks().len(), 2);
    assert_eq!(ed.project().timeline().tracks()[0].id(), tid(1));
    assert_eq!(ed.project().timeline().tracks()[1].kind(), TrackKind::Audio);
    assert_eq!(
        ed.apply(EditCommand::AddTrack {
            id: tid(1),
            name: "dup".into(),
            kind: TrackKind::Video,
        }),
        Err(EditorError::DuplicateId)
    );
    assert_eq!(
        ed.apply(EditCommand::AddTrack {
            id: tid(9),
            name: "".into(),
            kind: TrackKind::Video,
        }),
        Err(EditorError::EmptyName)
    );
    ed.apply(EditCommand::RemoveTrack { id: tid(1) })
        .expect("remove");
    assert_eq!(ed.project().timeline().tracks().len(), 1);
    assert_eq!(ed.project().timeline().tracks()[0].id(), tid(2));
}

#[test]
fn clips_insert_adjacency_overlap_cross_track_and_errors() {
    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    add_video(&mut ed, tid(2), "V2");

    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 10).expect("c1");
    insert_clip(&mut ed, cid(2), tid(1), aid(1), 10, 5).expect("adjacent");
    assert_eq!(
        insert_clip(&mut ed, cid(3), tid(1), aid(1), 8, 4),
        Err(EditorError::Overlap)
    );
    insert_clip(&mut ed, cid(3), tid(2), aid(1), 0, 100).expect("other track");
    assert_eq!(
        insert_clip(&mut ed, cid(1), tid(2), aid(1), 200, 1),
        Err(EditorError::DuplicateId)
    );
    assert_eq!(
        insert_clip(&mut ed, cid(9), tid(1), aid(99), 50, 1),
        Err(EditorError::AssetNotFound)
    );
    assert_eq!(
        TimelineDuration::from_micros(0),
        Err(EditorError::ZeroDuration)
    );
}

#[test]
fn move_trim_split_delete_and_atomicity() {
    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    add_video(&mut ed, tid(2), "V2");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 20).expect("c1");
    insert_clip(&mut ed, cid(2), tid(1), aid(1), 30, 10).expect("c2");

    ed.apply(EditCommand::MoveClip {
        id: cid(1),
        track_id: tid(1),
        start: pos(5),
    })
    .expect("move");
    assert_eq!(
        ed.project().timeline().tracks()[0].clips()[0].start(),
        pos(5)
    );

    let snapshot = ed.project().clone();
    let hist = ed.history().undo_len();
    assert_eq!(
        ed.apply(EditCommand::MoveClip {
            id: cid(1),
            track_id: tid(1),
            start: pos(28),
        }),
        Err(EditorError::Overlap)
    );
    assert_eq!(ed.project(), &snapshot);
    assert_eq!(ed.history().undo_len(), hist);

    ed.apply(EditCommand::MoveClip {
        id: cid(1),
        track_id: tid(2),
        start: pos(0),
    })
    .expect("cross track");
    assert_eq!(ed.project().timeline().tracks()[1].clips()[0].id(), cid(1));

    ed.apply(EditCommand::AddTrack {
        id: tid(3),
        name: "A1".into(),
        kind: TrackKind::Audio,
    })
    .expect("audio");
    assert_eq!(
        ed.apply(EditCommand::MoveClip {
            id: cid(1),
            track_id: tid(3),
            start: pos(0),
        }),
        Err(EditorError::IncompatibleTrackKind)
    );

    ed.apply(EditCommand::TrimClip {
        id: cid(2),
        start: pos(30),
        source_offset: off(2),
        duration: dur(8),
    })
    .expect("trim");
    let c2 = &ed.project().timeline().tracks()[0].clips()[0];
    assert_eq!(c2.duration(), dur(8));
    assert_eq!(c2.source_offset(), off(2));
}

#[test]
fn trim_rejected_when_overlapping_is_atomic() {
    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 10).expect("c1");
    insert_clip(&mut ed, cid(2), tid(1), aid(1), 20, 10).expect("c2");
    let snap = ed.project().clone();
    let hist = ed.history().undo_len();
    assert_eq!(
        ed.apply(EditCommand::TrimClip {
            id: cid(1),
            start: pos(0),
            source_offset: off(0),
            duration: dur(25),
        }),
        Err(EditorError::Overlap)
    );
    assert_eq!(ed.project(), &snap);
    assert_eq!(ed.history().undo_len(), hist);
}

#[test]
fn split_interior_and_edges() {
    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 100, 50).expect("c1");

    assert_eq!(
        ed.apply(EditCommand::SplitClip {
            id: cid(1),
            at: pos(100),
            right_id: cid(2),
        }),
        Err(EditorError::SplitNotInterior)
    );
    assert_eq!(
        ed.apply(EditCommand::SplitClip {
            id: cid(1),
            at: pos(150),
            right_id: cid(2),
        }),
        Err(EditorError::SplitNotInterior)
    );

    ed.apply(EditCommand::SplitClip {
        id: cid(1),
        at: pos(120),
        right_id: cid(2),
    })
    .expect("split");
    let clips = ed.project().timeline().tracks()[0].clips();
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0].id(), cid(1));
    assert_eq!(clips[0].duration(), dur(20));
    assert_eq!(clips[0].source_offset(), off(0));
    assert_eq!(clips[1].id(), cid(2));
    assert_eq!(clips[1].start(), pos(120));
    assert_eq!(clips[1].duration(), dur(30));
    assert_eq!(clips[1].source_offset(), off(20));
}

#[test]
fn selection_rules_and_invalid() {
    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 10).expect("c1");

    ed.set_selection(Selection::MediaAsset(aid(1)))
        .expect("sel asset");
    ed.set_selection(Selection::Track(tid(1)))
        .expect("sel track");
    ed.set_selection(Selection::Clip(cid(1))).expect("sel clip");
    assert_eq!(
        ed.set_selection(Selection::Clip(cid(9))),
        Err(EditorError::InvalidSelection)
    );

    ed.apply(EditCommand::DeleteClip { id: cid(1) })
        .expect("delete");
    assert_eq!(ed.project().selection(), Selection::None);

    insert_clip(&mut ed, cid(2), tid(1), aid(1), 0, 5).expect("c2");
    ed.set_selection(Selection::Clip(cid(2))).expect("sel");
    ed.apply(EditCommand::RemoveTrack { id: tid(1) })
        .expect("rm track");
    assert_eq!(ed.project().selection(), Selection::None);

    ed.set_selection(Selection::MediaAsset(aid(1)))
        .expect("sel asset");
    ed.apply(EditCommand::UnregisterAsset { id: aid(1) })
        .expect("unreg");
    assert_eq!(ed.project().selection(), Selection::None);
}

#[test]
fn undo_redo_insert_move_trim_split_delete_and_clear_redo() {
    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    add_video(&mut ed, tid(2), "V2");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 40).expect("insert");
    let after_insert = ed.project().clone();

    ed.apply(EditCommand::MoveClip {
        id: cid(1),
        track_id: tid(2),
        start: pos(10),
    })
    .expect("move");
    ed.undo().expect("undo move");
    assert_eq!(
        ed.project().timeline().tracks()[0].clips()[0].start(),
        pos(0)
    );
    ed.redo().expect("redo move");
    assert_eq!(
        ed.project().timeline().tracks()[1].clips()[0].start(),
        pos(10)
    );

    ed.apply(EditCommand::TrimClip {
        id: cid(1),
        start: pos(10),
        source_offset: off(5),
        duration: dur(30),
    })
    .expect("trim");
    ed.undo().expect("undo trim");
    assert_eq!(
        ed.project().timeline().tracks()[1].clips()[0].duration(),
        dur(40)
    );
    ed.redo().expect("redo trim");

    ed.apply(EditCommand::SplitClip {
        id: cid(1),
        at: pos(20),
        right_id: cid(2),
    })
    .expect("split");
    assert_eq!(ed.project().timeline().tracks()[1].clips().len(), 2);
    ed.undo().expect("undo split");
    assert_eq!(ed.project().timeline().tracks()[1].clips().len(), 1);
    assert_eq!(
        ed.project().timeline().tracks()[1].clips()[0].duration(),
        dur(30)
    );
    ed.redo().expect("redo split");
    assert_eq!(ed.project().timeline().tracks()[1].clips().len(), 2);

    ed.set_selection(Selection::Clip(cid(2))).expect("sel");
    ed.apply(EditCommand::DeleteClip { id: cid(2) })
        .expect("delete");
    assert_eq!(ed.project().selection(), Selection::None);
    ed.undo().expect("undo delete");
    assert_eq!(ed.project().selection(), Selection::Clip(cid(2)));
    ed.redo().expect("redo delete");

    // New command clears redo.
    ed.undo().expect("undo delete again");
    assert!(ed.history().can_redo());
    ed.apply(EditCommand::MoveClip {
        id: cid(2),
        track_id: tid(2),
        start: pos(25),
    })
    .expect("new cmd");
    assert!(!ed.history().can_redo());

    // Combined sequence back toward insert baseline shape.
    while ed.history().can_undo() {
        ed.undo().expect("undo");
    }
    assert_eq!(ed.project().media_pool().len(), 0);
    assert!(ed.project().timeline().tracks().is_empty());

    // Replay insert path markers: redo all
    while ed.history().can_redo() {
        ed.redo().expect("redo");
    }
    assert!(ed.project().media_pool().contains(aid(1)));
    let _ = after_insert;
}

#[test]
fn failed_command_does_not_alter_history_and_empty_undo_redo() {
    let mut ed = editor();
    assert_eq!(ed.undo(), Err(EditorError::NothingToUndo));
    assert_eq!(ed.redo(), Err(EditorError::NothingToRedo));
    register_asset(&mut ed, aid(1));
    let hist = ed.history().undo_len();
    let snap = ed.project().clone();
    assert_eq!(
        ed.apply(EditCommand::RegisterAsset { id: aid(1) }),
        Err(EditorError::DuplicateId)
    );
    assert_eq!(ed.history().undo_len(), hist);
    assert_eq!(ed.project(), &snap);
}

#[test]
fn noop_move_does_not_enter_history() {
    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 5, 10).expect("insert");
    let hist = ed.history().undo_len();
    ed.apply(EditCommand::MoveClip {
        id: cid(1),
        track_id: tid(1),
        start: pos(5),
    })
    .expect("noop");
    assert_eq!(ed.history().undo_len(), hist);
}

#[test]
fn rejected_command_after_undo_preserves_redo() {
    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 10).expect("insert");
    ed.undo().expect("undo insert");
    assert!(ed.history().can_redo());
    let redo_len = ed.history().redo_len();
    let undo_len = ed.history().undo_len();
    let snap = ed.project().clone();
    assert_eq!(
        ed.apply(EditCommand::RegisterAsset { id: aid(1) }),
        Err(EditorError::DuplicateId)
    );
    assert_eq!(ed.history().redo_len(), redo_len);
    assert_eq!(ed.history().undo_len(), undo_len);
    assert!(ed.history().can_redo());
    assert_eq!(ed.project(), &snap);
}

#[test]
fn unregister_asset_undo_redo_restores_selection() {
    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    ed.set_selection(Selection::MediaAsset(aid(1)))
        .expect("select");
    ed.apply(EditCommand::UnregisterAsset { id: aid(1) })
        .expect("unregister");
    assert_eq!(ed.project().selection(), Selection::None);
    ed.undo().expect("undo unregister");
    assert!(ed.project().media_pool().contains(aid(1)));
    assert_eq!(ed.project().selection(), Selection::MediaAsset(aid(1)));
    ed.redo().expect("redo unregister");
    assert!(!ed.project().media_pool().contains(aid(1)));
    assert_eq!(ed.project().selection(), Selection::None);
}

#[test]
fn failed_undo_restores_entry_and_project() {
    use crate::history::{ClipTiming, HistoryEntry};

    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    add_video(&mut ed, tid(2), "V2");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 10).expect("c1");
    // Occupies destination of the forged undo move.
    insert_clip(&mut ed, cid(2), tid(1), aid(1), 20, 10).expect("blocker");
    insert_clip(&mut ed, cid(3), tid(2), aid(1), 0, 10).expect("c3 on t2");

    let snap = ed.project().clone();
    let undo_before = ed.history().undo_len();
    let redo_before = ed.history().redo_len();

    // Forged entry: claims cid(3) was moved from track1@0 to track2@0, but
    // restoring to track1@0 overlaps cid(1).
    ed.inject_undo_entry_for_test(HistoryEntry::MoveClip {
        id: cid(3),
        from_track: tid(1),
        to_track: tid(2),
        from: ClipTiming {
            start: pos(0),
            source_offset: off(0),
            duration: dur(10),
        },
        to: ClipTiming {
            start: pos(0),
            source_offset: off(0),
            duration: dur(10),
        },
        selection_before: Selection::None,
    });

    assert_eq!(ed.undo(), Err(EditorError::Overlap));
    assert_eq!(ed.project(), &snap);
    assert_eq!(ed.history().undo_len(), undo_before + 1);
    assert_eq!(ed.history().redo_len(), redo_before);
    assert_eq!(ed.undo(), Err(EditorError::Overlap));
    assert_eq!(ed.history().undo_len(), undo_before + 1);
}

#[test]
fn failed_redo_restores_entry_and_project() {
    use crate::history::{ClipTiming, HistoryEntry};

    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    add_video(&mut ed, tid(2), "V2");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 10).expect("c1");
    insert_clip(&mut ed, cid(2), tid(2), aid(1), 0, 10).expect("c2");

    let snap = ed.project().clone();
    let undo_before = ed.history().undo_len();
    let redo_before = ed.history().redo_len();

    // Forged redo: move cid(1) from t1 to t2@0, but t2@0 is occupied.
    ed.inject_redo_entry_for_test(HistoryEntry::MoveClip {
        id: cid(1),
        from_track: tid(1),
        to_track: tid(2),
        from: ClipTiming {
            start: pos(0),
            source_offset: off(0),
            duration: dur(10),
        },
        to: ClipTiming {
            start: pos(0),
            source_offset: off(0),
            duration: dur(10),
        },
        selection_before: Selection::None,
    });

    assert_eq!(ed.redo(), Err(EditorError::Overlap));
    assert_eq!(ed.project(), &snap);
    assert_eq!(ed.history().redo_len(), redo_before + 1);
    assert_eq!(ed.history().undo_len(), undo_before);
    assert_eq!(ed.redo(), Err(EditorError::Overlap));
    assert_eq!(ed.history().redo_len(), redo_before + 1);
}

#[test]
fn history_move_overlap_rolls_back_clip() {
    use crate::history::{ClipTiming, HistoryEntry};

    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    add_video(&mut ed, tid(2), "V2");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 20).expect("blocker");
    insert_clip(&mut ed, cid(2), tid(2), aid(1), 5, 10).expect("moving");
    ed.set_selection(Selection::Clip(cid(2))).expect("sel");

    let snap = ed.project().clone();
    ed.inject_undo_entry_for_test(HistoryEntry::MoveClip {
        id: cid(2),
        from_track: tid(1),
        to_track: tid(2),
        from: ClipTiming {
            start: pos(0),
            source_offset: off(0),
            duration: dur(10),
        },
        to: ClipTiming {
            start: pos(5),
            source_offset: off(0),
            duration: dur(10),
        },
        selection_before: Selection::Clip(cid(2)),
    });

    assert_eq!(ed.undo(), Err(EditorError::Overlap));
    assert_eq!(ed.project(), &snap);
    let clips_t2 = ed.project().timeline().tracks()[1].clips();
    assert_eq!(clips_t2.len(), 1);
    assert_eq!(clips_t2[0].id(), cid(2));
    assert_eq!(clips_t2[0].start(), pos(5));
    assert_eq!(ed.project().selection(), Selection::Clip(cid(2)));
}

#[test]
fn history_trim_overlap_rolls_back_timing() {
    use crate::history::{ClipTiming, HistoryEntry};

    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 10).expect("c1");
    insert_clip(&mut ed, cid(2), tid(1), aid(1), 20, 10).expect("c2");

    let snap = ed.project().clone();
    ed.inject_undo_entry_for_test(HistoryEntry::TrimClip {
        id: cid(1),
        track_id: tid(1),
        before: ClipTiming {
            start: pos(0),
            source_offset: off(0),
            duration: dur(25),
        },
        after: ClipTiming {
            start: pos(0),
            source_offset: off(0),
            duration: dur(10),
        },
        selection_before: Selection::None,
    });

    assert_eq!(ed.undo(), Err(EditorError::Overlap));
    assert_eq!(ed.project(), &snap);
    let clips = ed.project().timeline().tracks()[0].clips();
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0].id(), cid(1));
    assert_eq!(clips[0].duration(), dur(10));
}

#[test]
fn split_rejected_when_right_would_overlap_is_atomic() {
    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 40).expect("c1");
    ed.set_selection(Selection::Clip(cid(1))).expect("sel");

    let snap = ed.project().clone();
    let hist_u = ed.history().undo_len();
    let hist_r = ed.history().redo_len();

    // Fail only the second insert_clip in this command (left then right).
    crate::timeline::fail_next_clip_insert::arm_after(1);

    assert_eq!(
        ed.apply(EditCommand::SplitClip {
            id: cid(1),
            at: pos(20),
            right_id: cid(3),
        }),
        Err(EditorError::Overlap)
    );
    assert_eq!(ed.project(), &snap);
    assert_eq!(ed.history().undo_len(), hist_u);
    assert_eq!(ed.history().redo_len(), hist_r);
    let clips = ed.project().timeline().tracks()[0].clips();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].id(), cid(1));
    assert_eq!(clips[0].duration(), dur(40));
    assert_eq!(ed.project().selection(), Selection::Clip(cid(1)));
}

#[test]
fn split_redo_second_insert_failure_rolls_back_left() {
    use crate::history::{ClipTiming, HistoryEntry};
    use crate::timeline::Clip;

    let mut ed = editor();
    register_asset(&mut ed, aid(1));
    add_video(&mut ed, tid(1), "V1");
    insert_clip(&mut ed, cid(1), tid(1), aid(1), 0, 40).expect("full left");

    let snap = ed.project().clone();
    let right = Clip::new(cid(2), aid(1), pos(20), off(20), dur(20));
    ed.inject_redo_entry_for_test(HistoryEntry::SplitClip {
        track_id: tid(1),
        left_id: cid(1),
        left_before: ClipTiming {
            start: pos(0),
            source_offset: off(0),
            duration: dur(40),
        },
        left_after: ClipTiming {
            start: pos(0),
            source_offset: off(0),
            duration: dur(20),
        },
        right,
        selection_before: Selection::None,
    });

    // set_clip_timing reinserts left (1), then right insert fails (2nd).
    crate::timeline::fail_next_clip_insert::arm_after(1);

    assert_eq!(ed.redo(), Err(EditorError::Overlap));
    assert_eq!(ed.project(), &snap);
    let clips = ed.project().timeline().tracks()[0].clips();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].id(), cid(1));
    assert_eq!(clips[0].duration(), dur(40));
    assert_eq!(ed.history().redo_len(), 1);
}
