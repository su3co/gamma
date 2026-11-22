"""1x2 MMI splitter from gdsfactory's generic PDK.

gamma runs this script to regenerate build/mmi.gds and build/mmi.ports.json
whenever it changes. Edit the parameters below and watch the layout update.
"""

import gdsfactory as gf

from gamma_pic import write_layout

WIDTH_MMI = 2.5  # um
LENGTH_MMI = 5.5  # um
GAP_MMI = 0.25  # um, between the output tapers
LENGTH_TAPER = 10.0  # um


def mmi():
    gf.gpdk.PDK.activate()
    return gf.components.mmi1x2(width_mmi=WIDTH_MMI, length_mmi=LENGTH_MMI, gap_mmi=GAP_MMI, length_taper=LENGTH_TAPER)


if __name__ == "__main__":
    write_layout(mmi())
