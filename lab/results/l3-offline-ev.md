# L3 offline EV lab

**Decision:** IMPLEMENT choose_action

L3 offline: candidates=['Wait-if-program-fanout', 'Bind-if-ready']. Hot morphology EV win without quiet degradation → authorize choose_action v3.

## Per-morphology wasteΔ vs M-A (negative = better)

| Morphology | Policy | mean wasteΔ | redo_saved | wait_added | win_frac |
|------------|--------|------------:|-----------:|-----------:|---------:|
| fan_out | M-D | -319.03 | 426.71 | 107.68 | 1.00 |
| fan_out | Wait-if-program-fanout | -382.11 | 467.76 | 85.65 | 1.00 |
| fan_out | Bind-if-ready | -413.13 | 454.27 | 41.14 | 1.00 |
| long_chain | M-D | -81.99 | 87.68 | 5.69 | 1.00 |
| long_chain | Wait-if-program-fanout | -88.16 | 97.36 | 9.19 | 1.00 |
| long_chain | Bind-if-ready | -126.95 | 132.69 | 5.74 | 1.00 |
| mixed | M-D | -31.17 | 38.38 | 7.21 | 1.00 |
| mixed | Wait-if-program-fanout | 0.00 | 0.00 | 0.00 | 0.00 |
| mixed | Bind-if-ready | -79.92 | 84.48 | 4.56 | 1.00 |
| quiet | M-D | -2.74 | 3.43 | 0.68 | 0.50 |
| quiet | Wait-if-program-fanout | 0.00 | 0.00 | 0.00 | 0.00 |
| quiet | Bind-if-ready | -6.24 | 6.37 | 0.13 | 1.00 |
| waw_spine | M-D | -129.53 | 133.91 | 4.38 | 1.00 |
| waw_spine | Wait-if-program-fanout | 0.00 | 0.00 | 0.00 | 0.00 |
| waw_spine | Bind-if-ready | -140.77 | 149.21 | 8.44 | 1.00 |

## Per-block headlines

- **14689597** (fan_out): raw=647 fanout=448 gw_p50=0.943 ready_done=0.710
  - M-D: redo_saved=426.7 wait_added=107.7 wasteΔ=-319.0 win=True
  - Wait-if-program-fanout: redo_saved=467.8 wait_added=85.6 wasteΔ=-382.1 win=True
  - Bind-if-ready: redo_saved=454.3 wait_added=41.1 wasteΔ=-413.1 win=True
- **14689599** (quiet): raw=1 fanout=0 gw_p50=0.000 ready_done=1.000
  - M-D: redo_saved=0.0 wait_added=0.0 wasteΔ=0.0 win=False
  - Wait-if-program-fanout: redo_saved=0.0 wait_added=0.0 wasteΔ=0.0 win=False
  - Bind-if-ready: redo_saved=1.0 wait_added=0.0 wasteΔ=-1.0 win=True
- **19469096** (waw_spine): raw=232 fanout=12 gw_p50=0.925 ready_done=0.850
  - M-D: redo_saved=133.9 wait_added=4.4 wasteΔ=-129.5 win=True
  - Wait-if-program-fanout: redo_saved=0.0 wait_added=0.0 wasteΔ=0.0 win=False
  - Bind-if-ready: redo_saved=149.2 wait_added=8.4 wasteΔ=-140.8 win=True
- **19469097** (long_chain): raw=410 fanout=6 gw_p50=0.611 ready_done=0.909
  - M-D: redo_saved=87.7 wait_added=5.7 wasteΔ=-82.0 win=True
  - Wait-if-program-fanout: redo_saved=97.4 wait_added=9.2 wasteΔ=-88.2 win=True
  - Bind-if-ready: redo_saved=132.7 wait_added=5.7 wasteΔ=-127.0 win=True
- **19606598** (quiet): raw=44 fanout=6 gw_p50=0.539 ready_done=1.000
  - M-D: redo_saved=6.9 wait_added=1.4 wasteΔ=-5.5 win=True
  - Wait-if-program-fanout: redo_saved=0.0 wait_added=0.0 wasteΔ=0.0 win=False
  - Bind-if-ready: redo_saved=11.8 wait_added=0.2 wasteΔ=-11.5 win=True
- **19606599** (mixed): raw=584 fanout=14 gw_p50=0.381 ready_done=0.874
  - M-D: redo_saved=38.4 wait_added=7.2 wasteΔ=-31.2 win=True
  - Wait-if-program-fanout: redo_saved=0.0 wait_added=0.0 wasteΔ=0.0 win=False
  - Bind-if-ready: redo_saved=84.5 wait_added=4.6 wasteΔ=-79.9 win=True

## Gate

```json
{
  "hot_morphologies_tested": [
    "fan_out",
    "long_chain",
    "mixed",
    "waw_spine"
  ],
  "quiet_morphologies_tested": [
    "quiet"
  ],
  "per_policy": {
    "M-D": {
      "hot_ev_win_any": true,
      "hot_ev_win_all": true,
      "quiet_not_degraded": true,
      "implement_choose_action_candidate": true
    },
    "Wait-if-program-fanout": {
      "hot_ev_win_any": true,
      "hot_ev_win_all": false,
      "quiet_not_degraded": true,
      "implement_choose_action_candidate": true
    },
    "Bind-if-ready": {
      "hot_ev_win_any": true,
      "hot_ev_win_all": true,
      "quiet_not_degraded": true,
      "implement_choose_action_candidate": true
    }
  }
}
```

