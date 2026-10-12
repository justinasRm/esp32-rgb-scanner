// Poll only the small RGB reading; this page never downloads camera images.
const info = document.getElementById("status");
const swatch = document.getElementById("swatch");
const hexLabel = document.getElementById("hex");
const rgbLabel = document.getElementById("rgb");

async function update() {
	try {
		const response = await fetch(`/rgb?t=${Date.now()}`, { cache: "no-store" });
		if (!response.ok) throw new Error("Color reading is not ready");
		const { red, green, blue } = await response.json();
		const hex =
			"#" +
			[red, green, blue]
				.map((value) => value.toString(16).padStart(2, "0"))
				.join("")
				.toUpperCase();

		swatch.style.backgroundColor = hex;
		hexLabel.textContent = hex;
		rgbLabel.textContent = `RGB(${red},${green},${blue})`;
		info.textContent = "Connected";
	} catch {
		info.textContent = "Waiting for camera…";
	}

	setTimeout(update, 1000);
}

update();
