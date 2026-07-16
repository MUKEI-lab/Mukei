package ai.mukei.android.designsystem

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp

@Composable
fun MukeiMark(
    modifier: Modifier = Modifier,
    contentDescription: String? = "Mukei mark",
) {
    Image(
        painter = painterResource(R.drawable.mukei_brand_mark),
        contentDescription = contentDescription,
        modifier = modifier.size(width = 80.dp, height = 88.dp),
        contentScale = ContentScale.Fit,
    )
}
