package ai.repose.blespike

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.view.View
import kotlin.math.min

/**
 * A ring that empties as a leg's 20 s run out, with the seconds left in the
 * middle (design doc §05: a countdown, not an elapsed 「N 秒了」).
 *
 * Fed by the calibration screen's ticker rather than by an animator of its own:
 * the number it shows is [CalFlow.remainingMs], which is the same clock the
 * decision to walk back is made on. Two clocks would eventually disagree by a
 * second, and the screen would walk you back while still showing 1.
 *
 * With nothing to count (the Mac has not confirmed the leg yet) the ring is
 * full and the middle shows a dash: the phone is waiting for the Mac, and a
 * countdown that had not started must not look like one that had.
 */
class CountdownView(context: Context, private val pal: Palette) : View(context) {

    private var remainingMs: Long? = null
    private var totalMs: Long = CalFlow.LEG_MS

    private val track = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = context.dpF(9f)
        strokeCap = Paint.Cap.ROUND
        color = pal.divider
    }
    private val arc = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = context.dpF(9f)
        strokeCap = Paint.Cap.ROUND
        color = pal.accent
    }
    private val number = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        textAlign = Paint.Align.CENTER
        textSize = context.dpF(44f)
        color = pal.textPrimary
        isFakeBoldText = true
    }
    private val unit = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        textAlign = Paint.Align.CENTER
        textSize = context.dpF(13f)
        color = pal.textSecondary
    }
    private val bounds = RectF()

    /** Called every tick. Null means "not counting yet". */
    fun set(remainingMs: Long?, totalMs: Long = CalFlow.LEG_MS) {
        val changed = this.remainingMs != remainingMs || this.totalMs != totalMs
        this.remainingMs = remainingMs
        this.totalMs = totalMs
        if (changed) invalidate()
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val side = context.dp(168)
        val w = resolveSize(side, widthMeasureSpec)
        setMeasuredDimension(w, side)
    }

    override fun onDraw(canvas: Canvas) {
        val side = min(width, height).toFloat()
        val inset = track.strokeWidth
        val left = (width - side) / 2f + inset
        val top = (height - side) / 2f + inset
        bounds.set(left, top, left + side - 2 * inset, top + side - 2 * inset)
        canvas.drawOval(bounds, track)

        val left_ = remainingMs
        val fraction = if (left_ == null || totalMs <= 0) 1f else (left_.toFloat() / totalMs).coerceIn(0f, 1f)
        if (fraction > 0f) {
            // From the top, clockwise: the way a clock face empties.
            canvas.drawArc(bounds, -90f, 360f * fraction, false, arc)
        }

        val cx = width / 2f
        val cy = height / 2f
        val label = if (left_ == null) "–" else ((left_ + 999) / 1000).toString()
        canvas.drawText(label, cx, cy + number.textSize * 0.28f, number)
        canvas.drawText(if (left_ == null) "等 Mac" else "秒", cx, cy + number.textSize * 0.28f + unit.textSize * 1.6f, unit)
    }
}
