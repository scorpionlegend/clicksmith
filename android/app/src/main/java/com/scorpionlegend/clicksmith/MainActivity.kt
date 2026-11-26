package com.scorpionlegend.clicksmith

import android.app.Activity
import android.app.AlertDialog
import android.content.ComponentName
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.Switch
import android.widget.TextView
import android.widget.Toast

class MainActivity : Activity() {
    private lateinit var intervalInput: EditText
    private lateinit var randomOffsetInput: EditText
    private lateinit var repeatCountInput: EditText
    private lateinit var repeatForeverSwitch: Switch
    private lateinit var tapsPerCycleSpinner: Spinner
    private lateinit var serviceStatus: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val preferences = getSharedPreferences(PREFERENCES, MODE_PRIVATE)
        val scrollView = ScrollView(this)
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(24), dp(28), dp(24), dp(32))
            setBackgroundColor(Color.rgb(244, 246, 250))
        }
        scrollView.addView(content)

        content.addView(TextView(this).apply {
            text = getString(R.string.app_name)
            textSize = 30f
            setTextColor(Color.rgb(23, 34, 53))
            typeface = android.graphics.Typeface.DEFAULT_BOLD
        }, fullWidth())
        content.addView(TextView(this).apply {
            text = getString(R.string.app_subtitle)
            textSize = 16f
            setTextColor(Color.rgb(75, 85, 99))
        }, bottomMargin(20))

        serviceStatus = TextView(this).apply {
            textSize = 15f
            setTextColor(Color.rgb(23, 34, 53))
            setPadding(dp(14), dp(12), dp(14), dp(12))
            setBackgroundColor(Color.WHITE)
        }
        content.addView(serviceStatus, bottomMargin(12))
        content.addView(button(getString(R.string.enable_tap_controls)) {
            requestTapControlsAccess()
        }, bottomMargin(22))

        addSectionTitle(content, getString(R.string.tap_settings))
        intervalInput = addNumberField(
            content,
            getString(R.string.interval_label),
            preferences.getLong(KEY_INTERVAL, 100L).toString(),
        )
        randomOffsetInput = addNumberField(
            content,
            getString(R.string.random_variation_label),
            preferences.getLong(KEY_RANDOM_OFFSET, 0L).toString(),
        )
        tapsPerCycleSpinner = Spinner(this).apply {
            adapter = ArrayAdapter.createFromResource(
                this@MainActivity,
                R.array.tap_cycle_options,
                android.R.layout.simple_spinner_item,
            ).also {
                it.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item)
            }
            setSelection(preferences.getInt(KEY_TAPS_PER_CYCLE, 1).coerceIn(1, 3) - 1)
        }
        content.addView(fieldLabel(getString(R.string.taps_per_interval)), topMargin(10))
        content.addView(tapsPerCycleSpinner, bottomMargin(14))

        repeatForeverSwitch = Switch(this).apply {
            text = getString(R.string.repeat_until_stopped)
            isChecked = preferences.getBoolean(KEY_REPEAT_FOREVER, true)
        }
        content.addView(repeatForeverSwitch, bottomMargin(4))
        repeatCountInput = addNumberField(
            content,
            getString(R.string.number_of_intervals),
            preferences.getLong(KEY_REPEAT_COUNT, 1L).toString(),
        )
        repeatForeverSwitch.setOnCheckedChangeListener { _, repeatForever ->
            repeatCountInput.isEnabled = !repeatForever
            repeatCountInput.alpha = if (repeatForever) 0.5f else 1f
        }
        repeatForeverSwitch.isChecked = preferences.getBoolean(KEY_REPEAT_FOREVER, true)

        content.addView(button(getString(R.string.save_settings)) { saveSettings() }, topMargin(10))
        content.addView(TextView(this).apply {
            text = getString(R.string.tap_instructions)
            textSize = 14f
            setTextColor(Color.rgb(75, 85, 99))
            setPadding(0, dp(18), 0, 0)
        }, fullWidth())
        content.addView(TextView(this).apply {
            text = getString(R.string.privacy_policy)
            textSize = 14f
            setTextColor(Color.rgb(23, 105, 170))
            setPadding(0, dp(18), 0, dp(8))
            setOnClickListener {
                startActivity(
                    Intent(
                        Intent.ACTION_VIEW,
                        Uri.parse(getString(R.string.privacy_policy_url)),
                    ),
                )
            }
        }, fullWidth())

        setContentView(scrollView)
    }

    override fun onResume() {
        super.onResume()
        if (::serviceStatus.isInitialized) {
            val serviceEnabled = isTapServiceEnabled()
            serviceStatus.text = if (serviceEnabled) {
                getString(R.string.service_status_enabled)
            } else {
                getString(R.string.service_status_disabled)
            }
            val disclosureAccepted = getSharedPreferences(PREFERENCES, MODE_PRIVATE)
                .getBoolean(KEY_ACCESSIBILITY_DISCLOSURE_ACCEPTED, false)
            if (serviceEnabled && disclosureAccepted) {
                sendOverlayCommand(TapAccessibilityService.ACTION_SHOW_OVERLAY)
            }
        }
    }

    override fun onDestroy() {
        if (isFinishing && isTapServiceEnabled()) {
            sendOverlayCommand(TapAccessibilityService.ACTION_HIDE_OVERLAY)
        }
        super.onDestroy()
    }

    private fun requestTapControlsAccess() {
        val preferences = getSharedPreferences(PREFERENCES, MODE_PRIVATE)
        if (preferences.getBoolean(KEY_ACCESSIBILITY_DISCLOSURE_ACCEPTED, false)) {
            openAccessibilitySettings()
            return
        }

        AlertDialog.Builder(this)
            .setTitle(R.string.accessibility_disclosure_title)
            .setMessage(R.string.accessibility_disclosure_message)
            .setNegativeButton(R.string.accessibility_disclosure_decline, null)
            .setPositiveButton(R.string.accessibility_disclosure_accept) { _, _ ->
                preferences.edit()
                    .putBoolean(KEY_ACCESSIBILITY_DISCLOSURE_ACCEPTED, true)
                    .apply()
                openAccessibilitySettings()
            }
            .show()
    }

    private fun openAccessibilitySettings() {
        startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
    }

    private fun sendOverlayCommand(action: String) {
        sendBroadcast(Intent(action).setPackage(packageName))
    }

    private fun saveSettings() {
        val interval = intervalInput.text.toString().toLongOrNull()?.coerceIn(1L, 3_600_000L)
        val randomOffset = randomOffsetInput.text.toString().toLongOrNull()?.coerceIn(0L, 60_000L)
        val repeatCount = repeatCountInput.text.toString().toLongOrNull()?.coerceAtLeast(1L)
        if (interval == null || randomOffset == null || repeatCount == null) {
            Toast.makeText(this, R.string.invalid_settings, Toast.LENGTH_LONG).show()
            return
        }

        getSharedPreferences(PREFERENCES, MODE_PRIVATE).edit()
            .putLong(KEY_INTERVAL, interval)
            .putLong(KEY_RANDOM_OFFSET, randomOffset)
            .putLong(KEY_REPEAT_COUNT, repeatCount)
            .putBoolean(KEY_REPEAT_FOREVER, repeatForeverSwitch.isChecked)
            .putInt(KEY_TAPS_PER_CYCLE, tapsPerCycleSpinner.selectedItemPosition + 1)
            .apply()
        Toast.makeText(this, R.string.settings_saved, Toast.LENGTH_SHORT).show()
    }

    private fun isTapServiceEnabled(): Boolean {
        val enabledServices = Settings.Secure.getString(
            contentResolver,
            Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
        ).orEmpty()
        val serviceName = ComponentName(this, TapAccessibilityService::class.java).flattenToString()
        return enabledServices.split(':').any { it.equals(serviceName, ignoreCase = true) }
    }

    private fun addNumberField(parent: LinearLayout, label: String, value: String): EditText {
        parent.addView(fieldLabel(label), topMargin(10))
        return EditText(this).apply {
            inputType = android.text.InputType.TYPE_CLASS_NUMBER
            setSingleLine(true)
            setText(value)
            textSize = 16f
            setPadding(dp(12), dp(8), dp(12), dp(8))
        }.also { parent.addView(it, bottomMargin(12)) }
    }

    private fun addSectionTitle(parent: LinearLayout, title: String) {
        parent.addView(TextView(this).apply {
            text = title
            textSize = 20f
            setTextColor(Color.rgb(23, 34, 53))
            typeface = android.graphics.Typeface.DEFAULT_BOLD
        }, bottomMargin(6))
    }

    private fun fieldLabel(textValue: String): TextView = TextView(this).apply {
        text = textValue
        textSize = 14f
        setTextColor(Color.rgb(55, 65, 81))
    }

    private fun button(label: String, action: () -> Unit): Button =
        Button(this).apply {
            text = label
            setOnClickListener { action() }
        }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    private fun fullWidth(): LinearLayout.LayoutParams =
        LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)

    private fun bottomMargin(value: Int): LinearLayout.LayoutParams =
        fullWidth().apply { bottomMargin = dp(value) }

    private fun topMargin(value: Int): LinearLayout.LayoutParams =
        fullWidth().apply { topMargin = dp(value) }

    companion object {
        const val PREFERENCES = "clicksmith_settings"
        const val KEY_INTERVAL = "interval_ms"
        const val KEY_RANDOM_OFFSET = "random_offset_ms"
        const val KEY_REPEAT_FOREVER = "repeat_forever"
        const val KEY_REPEAT_COUNT = "repeat_count"
        const val KEY_TAPS_PER_CYCLE = "taps_per_cycle"
        const val KEY_TARGET_X = "target_x"
        const val KEY_TARGET_Y = "target_y"
        const val KEY_ACCESSIBILITY_DISCLOSURE_ACCEPTED = "accessibility_disclosure_accepted"
    }
}
