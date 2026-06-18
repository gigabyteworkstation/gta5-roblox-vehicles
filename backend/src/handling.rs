//! Parse GTA5 `handling.meta` (plain XML) for one vehicle's `CHandlingData`.
//!
//! handling.meta is a flat XML list of `<Item type="CHandlingData">` blocks.
//! Rather than pull in an XML crate we scan text: the fields we need are simple
//! `<fName value="N" />` scalars and `<vecName x= y= z= />` vectors, and each
//! vehicle's fields live between its `<handlingName>` tag and the next one.
//!
//! These are the inputs to RAGE's CWheel / CTransmission / suspension model —
//! the same numbers the game drives its vehicles with.

#[derive(Debug, Clone)]
pub struct Handling {
    pub mass: f32,
    pub drag_coeff: f32,
    pub com_offset: [f32; 3],
    pub inertia_mult: [f32; 3],
    pub drive_bias_front: f32,
    pub drive_gears: f32,
    pub drive_force: f32,
    pub drive_max_flat_vel: f32,
    pub brake_force: f32,
    pub brake_bias_front: f32,
    pub handbrake_force: f32,
    pub steering_lock: f32,
    pub traction_curve_max: f32,
    pub traction_curve_min: f32,
    pub traction_curve_lateral: f32,
    pub traction_bias_front: f32,
    pub low_speed_traction_loss: f32,
    pub suspension_force: f32,
    pub suspension_comp_damp: f32,
    pub suspension_rebound_damp: f32,
    pub suspension_upper_limit: f32,
    pub suspension_lower_limit: f32,
    pub suspension_raise: f32,
    pub suspension_bias_front: f32,
    pub anti_roll_force: f32,
    pub anti_roll_bias_front: f32,
    pub seat_offset: [f32; 3],
}

/// Scalar `<tag value="N" />` inside `block`.
fn fval(block: &str, tag: &str) -> Option<f32> {
    let needle = format!("<{tag} value=\"");
    let i = block.find(&needle)? + needle.len();
    let rest = &block[i..];
    let end = rest.find('"')?;
    rest[..end].trim().parse().ok()
}

fn fval_or(block: &str, tag: &str, default: f32) -> f32 {
    fval(block, tag).unwrap_or(default)
}

/// Vector `<tag x="X" y="Y" z="Z" />` inside `block`.
fn vec3(block: &str, tag: &str) -> [f32; 3] {
    let needle = format!("<{tag} ");
    let Some(i) = block.find(&needle) else { return [0.0; 3] };
    let rest = &block[i..];
    let end = rest.find("/>").unwrap_or(rest.len());
    let seg = &rest[..end];
    let attr = |a: &str| -> f32 {
        let n = format!("{a}=\"");
        seg.find(&n)
            .and_then(|p| {
                let s = &seg[p + n.len()..];
                s.find('"').and_then(|e| s[..e].trim().parse().ok())
            })
            .unwrap_or(0.0)
    };
    [attr("x"), attr("y"), attr("z")]
}

/// Look up a vehicle's handling block by `<handlingName>NAME</handlingName>`
/// (case-sensitive, exact tag so `ZION` won't match `ZION2` etc.).
pub fn parse(xml: &str, handling_name: &str) -> Option<Handling> {
    let needle = format!("<handlingName>{handling_name}</handlingName>");
    let start = xml.find(&needle)?;
    let rest = &xml[start..];
    // The block ends at the next vehicle's handlingName (or end of file).
    let end = rest[needle.len()..]
        .find("<handlingName>")
        .map(|i| needle.len() + i)
        .unwrap_or(rest.len());
    let b = &rest[..end];

    Some(Handling {
        mass: fval_or(b, "fMass", 1400.0),
        drag_coeff: fval_or(b, "fInitialDragCoeff", 5.5),
        com_offset: vec3(b, "vecCentreOfMassOffset"),
        inertia_mult: {
            let v = vec3(b, "vecInertiaMultiplier");
            if v == [0.0; 3] { [1.0, 1.0, 1.0] } else { v }
        },
        drive_bias_front: fval_or(b, "fDriveBiasFront", 0.0),
        drive_gears: fval_or(b, "nInitialDriveGears", 5.0),
        drive_force: fval_or(b, "fInitialDriveForce", 0.25),
        drive_max_flat_vel: fval_or(b, "fInitialDriveMaxFlatVel", 150.0),
        brake_force: fval_or(b, "fBrakeForce", 1.0),
        brake_bias_front: fval_or(b, "fBrakeBiasFront", 0.5),
        handbrake_force: fval_or(b, "fHandBrakeForce", 0.7),
        steering_lock: fval_or(b, "fSteeringLock", 35.0),
        traction_curve_max: fval_or(b, "fTractionCurveMax", 2.4),
        traction_curve_min: fval_or(b, "fTractionCurveMin", 2.2),
        traction_curve_lateral: fval_or(b, "fTractionCurveLateral", 22.5),
        traction_bias_front: fval_or(b, "fTractionBiasFront", 0.5),
        low_speed_traction_loss: fval_or(b, "fLowSpeedTractionLossMult", 1.0),
        suspension_force: fval_or(b, "fSuspensionForce", 2.5),
        suspension_comp_damp: fval_or(b, "fSuspensionCompDamp", 1.4),
        suspension_rebound_damp: fval_or(b, "fSuspensionReboundDamp", 3.0),
        suspension_upper_limit: fval_or(b, "fSuspensionUpperLimit", 0.08),
        suspension_lower_limit: fval_or(b, "fSuspensionLowerLimit", -0.1),
        suspension_raise: fval_or(b, "fSuspensionRaise", 0.0),
        suspension_bias_front: fval_or(b, "fSuspensionBiasFront", 0.5),
        anti_roll_force: fval_or(b, "fAntiRollBarForce", 0.0),
        anti_roll_bias_front: fval_or(b, "fAntiRollBarBiasFront", 0.5),
        seat_offset: [
            fval_or(b, "fSeatOffsetDistX", 0.0),
            fval_or(b, "fSeatOffsetDistY", 0.0),
            fval_or(b, "fSeatOffsetDistZ", 0.0),
        ],
    })
}
