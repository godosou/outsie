# Stretch guide human asset

`stretch-human.json` is derived from MakeHuman Community **core assets**, licensed CC0 1.0. No MakeHuman application source code is included.

- License explanation: https://static.makehumancommunity.org/about/license.html
- CC0 legal text: https://creativecommons.org/publicdomain/zero/1.0/legalcode
- Mesh: https://github.com/makehumancommunity/makehuman/blob/master/makehuman/data/3dobjs/base.obj
- Skeleton: https://github.com/makehumancommunity/makehuman/blob/master/makehuman/data/rigs/default.mhskel
- Weights: https://github.com/makehumancommunity/makehuman/blob/master/makehuman/data/rigs/default_weights.mhw
- Weights credit: Data Collection AB, Joel Palmius, Jonas Hauquier (2021).

Body-shape targets (also CC0 core assets): `makehuman/data/targets/macrodetails/caucasian-male-young.target` and `makehuman/data/targets/macrodetails/universal-male-young-maxmuscle-averageweight.target` in the same upstream repository.

Reproduction: download the three assets and two targets above, then run `node scripts/build-stretch-human.mjs <base.obj> <default.mhskel> <default_weights.mhw> <caucasian-male-young.target> <universal-male-young-maxmuscle-averageweight.target>`. The converter applies both adult shape targets, removes helpers, triangulates the body, retains the strongest four normalized skin weights and bakes a relaxed standing bind pose.

Muscle & Motion is a visual reference only. No model, image, animation, logo or other media from that product is distributed. Red overlays indicate approximate surface regions, not segmented anatomical muscles or medical/biomechanical validation.
