package com.scorpionlegend.clicksmith

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.WindowManager
import android.view.accessibility.AccessibilityEvent
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import kotlin.random.Random

class TapAccessibilityService : AccessibilityService() {
    private val handler = Handler(Looper.getMainLooper())
    private lateinit var windowManager: WindowManager
    private lateinit var targetView: TextView
    private lateinit var controlsView: LinearLayout
    private lateinit var statusView: TextView
    private lateinit var startButton: Button
    private lateinit var stopButton: Button
    private lateinit var targetParams: WindowManager.LayoutParams
    private var isRunning = false
    private var tapsInCycle = 0
    private var completedCycles = 0L
    private var settings: ClickSettings? = null

    override fun onServiceConnected() {
        super.onServiceConnected()
        windowManager = getSystemService(WINDOW_SERVICE) as WindowManager
        showFloatingControls()
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit

    override fun onInterrupt() {
        stopClicking(getString(R.string.tap_interrupted))
    }

    override fun onDestroy() {
        stopClicking(getString(R.string.tap_controls_stopped))
        if (::windowManager.isInitialized) {
            if (::targetView.isInitialized && targetView.isAttachedToWindow) windowManager.removeView(targetView)
            if (::controlsView.isInitialized && controlsView.isAttachedToWindow) windowManager.removeView(controlsView)
        }
        super.onDestroy()
    }

    private fun showFloatingControls() {
        val preferences = getSharedPreferences(MainActivity.PREFERENCES, Context.MODE_PRIVATE)
        val density = resources.displayMetrics.density
        val targetSize = (52 * density).toInt()

        targetView = TapTargetView(this).apply {
            text = "●"
            textSize = 30f
            gravity = Gravity.CENTER
            setTextColor(Color.WHITE)
            contentDescription = getString(R.string.tap_target_description)
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(0xCC4A86CA.toInt())
                setStroke((2 * density).toInt(), Color.WHITE)
            }
            setOnTouchListener(TargetDragListener())
        }

        val screenWidth = resources.displayMetrics.widthPixels
        val screenHeight = resources.displayMetrics.heightPixels
        val x = preferences.getInt(MainActivity.KEY_TARGET_X, screenWidth / 2)
        val y = preferences.getInt(MainActivity.KEY_TARGET_Y, screenHeight / 2)
        targetParams = WindowManager.LayoutParams(
            targetSize,
            targetSize,
            WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            this.x = (x - targetSize / 2).coerceIn(0, (screenWidth - targetSize).coerceAtLeast(0))
            this.y = (y - targetSize / 2).coerceIn(0, (screenHeight - targetSize).coerceAtLeast(0))
        }
        windowManager.addView(targetView, targetParams)

        statusView = TextView(this).apply {
            text = getString(R.string.service_ready)
            textSize = 12f
            setTextColor(Color.WHITE)
            gravity = Gravity.CENTER_VERTICAL
        }
        startButton = overlayButton(getString(R.string.start)) { startClicking() }
        stopButton = overlayButton(getString(R.string.stop)) { stopClicking(getString(R.string.stopped)) }
        controlsView = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding((10 * density).toInt(), (6 * density).toInt(), (10 * density).toInt(), (6 * density).toInt())
            background = GradientDrawable().apply {
                setColor(0xE6172235.toInt())
                cornerRadius = 18 * density
            }
            addView(statusView, LinearLayout.LayoutParams(-1, -2))
            addView(LinearLayout(this@TapAccessibilityService).apply {
                orientation = LinearLayout.HORIZONTAL
                addView(startButton)
                addView(stopButton)
                addView(overlayButton(getString(R.string.settings)) { openSettings() })
            })
        }

        val controlsParams = WindowManager.LayoutParams(
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            this.x = (12 * density).toInt()
            this.y = (48 * density).toInt()
        }
        windowManager.addView(controlsView, controlsParams)
    }

    private fun overlayButton(label: String, action: () -> Unit): Button =
        Button(this).apply {
            text = label
            textSize = 12f
            minWidth = 0
            minimumWidth = 0
            setPadding(8, 0, 8, 0)
            setOnClickListener { action() }
        }

    private fun openSettings() {
        startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    private fun startClicking() {
        if (isRunning) return
        settings = ClickSettings.load(this)
        tapsInCycle = 0
        completedCycles = 0
        isRunning = true
        targetParams.flags = WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
            WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN or
            WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE
        windowManager.updateViewLayout(targetView, targetParams)
        refreshControls(getString(R.string.running))
        dispatchNextTap()
    }

    private fun dispatchNextTap() {
        val currentSettings = settings ?: return
        if (!isRunning) return

        val location = IntArray(2)
        targetView.getLocationOnScreen(location)
        val tapX = location[0] + targetView.width / 2f
        val tapY = location[1] + targetView.height / 2f
        val path = Path().apply { moveTo(tapX, tapY) }
        val gesture = GestureDescription.Builder()
            .addStroke(GestureDescription.StrokeDescription(path, 0, 1))
            .build()

        val accepted = dispatchGesture(gesture, object : GestureResultCallback() {
            override fun onCompleted(gestureDescription: GestureDescription?) {
                if (!isRunning) return
                tapsInCycle += 1
                if (tapsInCycle < currentSettings.tapsPerCycle) {
                    handler.postDelayed({ dispatchNextTap() }, CLICK_GAP_MS)
                    return
                }

                tapsInCycle = 0
                completedCycles += 1
                val repeatCount = currentSettings.repeatCount
                if (repeatCount != null && completedCycles >= repeatCount) {
                    val quantity = if (completedCycles == 1L) 1 else 2
                    stopClicking(resources.getQuantityString(R.plurals.finished_intervals, quantity, completedCycles))
                } else {
                    handler.postDelayed({ dispatchNextTap() }, currentSettings.nextInterval())
                }
            }

            override fun onCancelled(gestureDescription: GestureDescription?) {
                if (isRunning) stopClicking(getString(R.string.tap_cancelled))
            }
        }, null)
        if (!accepted) stopClicking(getString(R.string.tap_rejected))
    }

    private fun stopClicking(message: String) {
        isRunning = false
        settings = null
        handler.removeCallbacksAndMessages(null)
        if (::targetParams.isInitialized && ::targetView.isInitialized && targetView.isAttachedToWindow) {
            targetParams.flags = WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN
            windowManager.updateViewLayout(targetView, targetParams)
        }
        if (::statusView.isInitialized) refreshControls(message)
    }

    private fun refreshControls(message: String) {
        statusView.text = message
        startButton.isEnabled = !isRunning
        stopButton.isEnabled = isRunning
    }

    private inner class TargetDragListener : View.OnTouchListener {
        private var startRawX = 0f
        private var startRawY = 0f
        private var startWindowX = 0
        private var startWindowY = 0
        private var moved = false

        override fun onTouch(view: View, event: MotionEvent): Boolean {
            when (event.action) {
                MotionEvent.ACTION_DOWN -> {
                    startRawX = event.rawX
                    startRawY = event.rawY
                    startWindowX = targetParams.x
                    startWindowY = targetParams.y
                    moved = false
                    return true
                }
                MotionEvent.ACTION_MOVE -> {
                    moved = true
                    targetParams.x = startWindowX + (event.rawX - startRawX).toInt()
                    targetParams.y = startWindowY + (event.rawY - startRawY).toInt()
                    windowManager.updateViewLayout(targetView, targetParams)
                    return true
                }
                MotionEvent.ACTION_UP -> {
                    if (!moved) view.performClick()
                    val location = IntArray(2)
                    targetView.getLocationOnScreen(location)
                    getSharedPreferences(MainActivity.PREFERENCES, Context.MODE_PRIVATE).edit()
                        .putInt(MainActivity.KEY_TARGET_X, location[0] + targetView.width / 2)
                        .putInt(MainActivity.KEY_TARGET_Y, location[1] + targetView.height / 2)
                        .apply()
                    return true
                }
            }
            return false
        }
    }

    companion object {
        private const val CLICK_GAP_MS = 55L
    }
}

private class TapTargetView(context: Context) : TextView(context) {
    override fun performClick(): Boolean {
        super.performClick()
        return true
    }
}
