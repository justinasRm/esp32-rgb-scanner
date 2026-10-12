// Fetch one camera frame and its ESP32-calculated color each second.

const canvas = document.getElementById("view");
const context = canvas.getContext("2d", { willReadFrequently: true });
const status = document.getElementById("status");
let running = true;
let timer;

async function update() {
	if (!running) return;

	try {
		const frameResponse = await fetch(`/capture?t=${Date.now()}`, {
			cache: "no-store",
		});
		if (!frameResponse.ok) throw new Error("Camera frame is not ready");
		const image = await createImageBitmap(await frameResponse.blob());
		canvas.width = image.width;
		canvas.height = image.height;
		context.drawImage(image, 0, 0);
		image.close();

		const colorResponse = await fetch(`/rgb?t=${Date.now()}`, {
			cache: "no-store",
		});
		if (!colorResponse.ok) throw new Error("Color reading is not ready");
		const color = await colorResponse.json();
		showColor(color);
		drawSampleRegion();
		status.textContent = `Camera frame · ${canvas.width} × ${canvas.height}`;
	} catch {
		status.textContent = "Waiting for camera…";
	}

	if (running) timer = setTimeout(update, 1000);
}

function showColor({ red, green, blue }) {
	const hex =
		"#" +
		[red, green, blue]
			.map((value) => value.toString(16).padStart(2, "0"))
			.join("")
			.toUpperCase();
	document.getElementById("hex").textContent = hex;
	document.getElementById("rgb").textContent =
		`R ${red} · G ${green} · B ${blue}`;
	document.getElementById("swatch").style.backgroundColor = hex;
}
function drawSampleRegion() {
	const side = Math.sqrt(0.05);
	const width = Math.round(canvas.width * side);
	const height = Math.round(canvas.height * side);
	const x = Math.floor((canvas.width - width) / 2);
	const y = Math.floor((canvas.height - height) / 2);
	context.strokeStyle = "white";
	context.lineWidth = Math.max(2, Math.round(canvas.width / 500));
	context.strokeRect(x, y, width, height);
}

document.getElementById("toggle").onclick = function () {
	running = !running;
	this.textContent = running ? "Pause" : "Resume";
	clearTimeout(timer);
	if (running) update();
};

document.getElementById("copy").onclick = () =>
	navigator.clipboard.writeText(document.getElementById("rgb").textContent);

update();
