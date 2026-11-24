package com.scorpionlegend.clicksmith

import android.content.Context
import kotlin.random.Random

internal data class ClickSettings(
    val intervalMs: Long,
    val randomOffsetMs: Long,
    val repeatCount: Long?,
    val tapsPerCycle: Int,
) {
    fun nextInterval(random: Random = Random.Default): Long {
        if (randomOffsetMs == 0L) return intervalMs
        return (intervalMs + random.nextLong(-randomOffsetMs, randomOffsetMs + 1))
            .coerceAtLeast(1L)
    }

    companion object {
        fun load(context: Context): ClickSettings {
            val preferences = context.getSharedPreferences(MainActivity.PREFERENCES, Context.MODE_PRIVATE)
            val interval = preferences.getLong(MainActivity.KEY_INTERVAL, 100L).coerceIn(1L, 3_600_000L)
            val offset = preferences.getLong(MainActivity.KEY_RANDOM_OFFSET, 0L).coerceIn(0L, 60_000L)
            val repeatCount = if (preferences.getBoolean(MainActivity.KEY_REPEAT_FOREVER, true)) {
                null
            } else {
                preferences.getLong(MainActivity.KEY_REPEAT_COUNT, 1L).coerceAtLeast(1L)
            }
            val tapsPerCycle = preferences.getInt(MainActivity.KEY_TAPS_PER_CYCLE, 1).coerceIn(1, 3)
            return ClickSettings(interval, offset, repeatCount, tapsPerCycle)
        }
    }
}
