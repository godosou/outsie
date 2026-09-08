# 3D stretch alignment review · v0.6.3

The accepted layout places an enlarged 3D guide on the left, with instructions, countdown and controls on the right. Before release, the owner requested a check of all eight movements against their written instructions.

| Movement | Finding and resulting behavior |
| --- | --- |
| Chin tuck | Retain a small backward head translation with level gaze; do not exaggerate it into a nod. |
| Side neck stretch | Reduce the combined neck/head tilt from about 30° to about 21°, without hand pressure. |
| Backward shoulder rolls | Correct the girdle sequence to up → back → down → forward; keep arms relaxed and describe a small circle. |
| Upper trapezius | Correct the active arm/head pairing: the arm reaches behind the hip and the head tilts away from that side. |
| Chest opener | Use a gentler backward arm reach and keep the spine neutral. |
| Upper-back rotation | Remove the unexplained raised-arm pose and lower-spine rotation; rotate the upper spine with arms down and pelvis fixed. |
| Wrist/forearm | Reach forward near shoulder height, keep the elbow slightly relaxed and flex the wrist downward; widen the view to retain the hand. |
| Standing side bend | Retain the opposite-side arm lift and planted feet; widen narrow-stage framing to retain fingers during the lateral arm-lift transition. |

## Verification

All eight representative poses were inspected in the browser beside their current instructions. Eight new regression tests reconstruct the actual bundled bone hierarchy and check world-space landmark relationships. They cover both sides, level chin retraction, gentle neck tilt, fixed pelvis/feet, arm direction, downward wrist flexion, and 32 samples across each motion cycle at four stage aspect ratios. The framing checks include both wrists and middle-finger landmarks. Existing playback pause/resume, motion reduction and timer tests remain applicable.

The automated checks establish animation/instruction consistency, not clinical effectiveness, precise muscle activation, or suitability for an individual user. The model remains a general movement illustration.

## Content references

- [Cambridge University Hospitals: Neck exercises and advice](https://www.cuh.nhs.uk/patient-information/neck-exercises-and-advice/): gentle side tilt and level chin retraction.
- [Mayo Clinic: Desk stretches](https://www.mayoclinic.org/healthy-lifestyle/adult-health/in-depth/office-stretches/art-20046041): gentle workplace activity.
- [NIH: Tired, Achy Eyes?](https://newsinhealth.nih.gov/2024/09/tired-achy-eyes): short-break eye-care content.
