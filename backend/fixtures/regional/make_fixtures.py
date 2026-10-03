"""Gera as fixtures FICTÍCIAS do Regional Intelligence.

Os arquivos seguem o formato documentado de cada fonte (GeoPackage do IBGE,
CSV dos agregados, CSV do IPVS, GeoJSON do SGB e do WFS do GeoSampa), mas
códigos, geometrias e valores são inventados para testes.

    python3 fixtures/regional/make_fixtures.py
"""

import io
import json
import os
import sqlite3
import struct
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))


def square(lon0, lat0, lon1, lat1):
    return [(lon0, lat0), (lon1, lat0), (lon1, lat1), (lon0, lat1), (lon0, lat0)]


def wkb_polygon(ring):
    out = struct.pack("<BII", 1, 3, 1) + struct.pack("<I", len(ring))
    for x, y in ring:
        out += struct.pack("<dd", x, y)
    return out


def gpkg_blob(wkb, srs_id, empty=False):
    # GP, versão 0, flags: little endian, sem envelope (+ bit de vazio).
    flags = 0b0000_0001 | (0b0001_0000 if empty else 0)
    return b"GP" + bytes([0, flags]) + struct.pack("<i", srs_id) + (b"" if empty else wkb)


# Setores fictícios: dois vizinhos em Rudge Ramos (anúncio fx-001), um no
# Centro de São Bernardo (fx-004) e um em São Paulo (ponto -23.55, -46.63).
SECTORS = [
    ("354870805000012", "São Bernardo do Campo", 0.36, square(-46.5760, -23.6590, -46.5700, -23.6530)),
    ("354870805000013", "São Bernardo do Campo", 0.36, square(-46.5700, -23.6590, -46.5640, -23.6530)),
    ("354870805000020", "São Bernardo do Campo", 0.36, square(-46.5680, -23.6970, -46.5620, -23.6910)),
    ("355030801000001", "São Paulo", 1.05, square(-46.6350, -23.5550, -46.6250, -23.5450)),
]


def make_gpkg():
    path = os.path.join(HERE, "ibge_setores_sample.gpkg")
    if os.path.exists(path):
        os.remove(path)
    db = sqlite3.connect(path)
    db.executescript(
        """
        CREATE TABLE gpkg_spatial_ref_sys (srs_name TEXT, srs_id INTEGER PRIMARY KEY,
            organization TEXT, organization_coordsys_id INTEGER, definition TEXT, description TEXT);
        CREATE TABLE gpkg_contents (table_name TEXT PRIMARY KEY, data_type TEXT, identifier TEXT,
            description TEXT, last_change TEXT, min_x REAL, min_y REAL, max_x REAL, max_y REAL, srs_id INTEGER);
        CREATE TABLE gpkg_geometry_columns (table_name TEXT, column_name TEXT, geometry_type_name TEXT,
            srs_id INTEGER, z INTEGER, m INTEGER);
        CREATE TABLE SP_setores_CD2022 (fid INTEGER PRIMARY KEY, geom BLOB, CD_SETOR TEXT,
            CD_MUN TEXT, NM_MUN TEXT, AREA_KM2 REAL);
        """
    )
    db.execute(
        "INSERT INTO gpkg_spatial_ref_sys VALUES ('SIRGAS 2000', 4674, 'EPSG', 4674, 'undefined', NULL)"
    )
    db.execute(
        "INSERT INTO gpkg_contents VALUES ('SP_setores_CD2022', 'features', 'SP_setores_CD2022', '', '', NULL, NULL, NULL, NULL, 4674)"
    )
    db.execute("INSERT INTO gpkg_geometry_columns VALUES ('SP_setores_CD2022', 'geom', 'MULTIPOLYGON', 4674, 0, 0)")
    for code, name, area, ring in SECTORS:
        db.execute(
            "INSERT INTO SP_setores_CD2022 (geom, CD_SETOR, CD_MUN, NM_MUN, AREA_KM2) VALUES (?, ?, ?, ?, ?)",
            (gpkg_blob(wkb_polygon(ring), 4674), code + "P", code[:7], name, area),
        )
    # Setor com geometria vazia e setor com código inválido: devem ser ignorados.
    db.execute(
        "INSERT INTO SP_setores_CD2022 (geom, CD_SETOR, CD_MUN, NM_MUN, AREA_KM2) VALUES (?, ?, ?, ?, ?)",
        (gpkg_blob(b"", 4674, empty=True), "354870805000099", "3548708", "São Bernardo do Campo", 0.1),
    )
    db.execute(
        "INSERT INTO SP_setores_CD2022 (geom, CD_SETOR, CD_MUN, NM_MUN, AREA_KM2) VALUES (?, ?, ?, ?, ?)",
        (gpkg_blob(wkb_polygon(SECTORS[0][3]), 4674), "3548708ABC", "3548708", "São Bernardo do Campo", 0.1),
    )
    db.commit()
    db.close()


def make_aggregates():
    rows = [
        "CD_SETOR;NM_MUN;V0001;V0002;V0003;V0004;V0005;V0006;V0007",
        "354870805000012;São Bernardo do Campo;512;190;189;1;2,88;0,5;178",
        "354870805000013;São Bernardo do Campo;X;X;X;X;X;X;X",
        "354870805000020;São Bernardo do Campo;730;310;310;0;2,45;0;298",
        "355030801000001;São Paulo;2100;900;898;2;2,51;1,1;837",
        "354870805000030;São Bernardo do Campo;abc;1;1;0;1;0;1",
    ]
    text = "\r\n".join(rows) + "\r\n"
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        # Latin-1, como em vários arquivos do IBGE.
        z.writestr("Agregados_por_setores_basico_BR.csv", text.encode("latin-1"))
    with open(os.path.join(HERE, "ibge_agregados_basico_sample.zip"), "wb") as f:
        f.write(buf.getvalue())


def make_ipvs():
    rows = [
        "cod_setor;municipio;grupo_ipvs",
        "354870805000012;São Bernardo do Campo;2",
        "354870805000013;São Bernardo do Campo;5",
        "355030801000001;São Paulo;1",
    ]
    with open(os.path.join(HERE, "seade_ipvs_sample.csv"), "w", encoding="utf-8") as f:
        f.write("\n".join(rows) + "\n")


def feature(fid, ring, props):
    return {
        "type": "Feature",
        "id": fid,
        "geometry": {"type": "Polygon", "coordinates": [[list(p) for p in ring]]},
        "properties": props,
    }


def make_sgb():
    base = {
        "uf": "SP", "munic": "SÃO BERNARDO DO CAMPO", "cd_geocmu": "3548708",
        "tipolo_e1": None, "tipolo_g3": None, "tipolo_e3": None, "cobrade_03": None,
        "grau_vulne": "Médio", "num_edif": 10.0, "num_domi": 12.0, "num_pess": 40.0,
        "orgao_exec": "Serviço Geológico do Brasil", "data_setor": 1400025600000,
    }
    features = [
        # Contém o anúncio fx-004 (-23.694, -46.565).
        feature(1, square(-46.5660, -23.6950, -46.5640, -23.6930), {
            **base, "objectid": 1, "num_setor": "SP_SBC_SR_FICT_01", "local": "Setor fictício 1",
            "tipolo_g1": "Deslizamento", "tipolo_e1": "Deslizamento planar", "cobrade_01": "1.1.3.2.1",
            "tipolo_g2": "Enxurrada", "tipolo_e2": None, "cobrade_02": "1.2.2.0.0",
            "grau_risco": "Muito Alto",
        }),
        # A ~330 m a leste do anúncio fx-001 (-23.656, -46.573).
        feature(2, square(-46.5698, -23.6570, -46.5680, -23.6550), {
            **base, "objectid": 2, "num_setor": "SP_SBC_SR_FICT_02", "local": "Setor fictício 2",
            "tipolo_g1": "Inundação", "cobrade_01": "1.2.1.0.0",
            "tipolo_g2": None, "tipolo_e2": None, "cobrade_02": None,
            "grau_risco": "Alto",
        }),
        # Sem geometria: deve ser ignorado.
        {"type": "Feature", "id": 3, "geometry": None, "properties": {
            **base, "objectid": 3, "num_setor": "SP_SBC_SR_FICT_03", "tipolo_g1": "Erosão",
            "cobrade_01": "1.1.4.2.0", "grau_risco": "Alto",
        }},
    ]
    with open(os.path.join(HERE, "sgb_risco_sample.geojson"), "w", encoding="utf-8") as f:
        json.dump({"type": "FeatureCollection", "features": features}, f, ensure_ascii=False, indent=1)


def make_geosampa():
    crs = {"type": "name", "properties": {"name": "urn:ogc:def:crs:EPSG::31983"}}

    def point(fid, x, y, props):
        return {"type": "Feature", "id": fid, "geometry": {"type": "Point", "coordinates": [x, y]},
                "properties": props}

    libraries = {"type": "FeatureCollection", "crs": crs, "features": [
        # ~230 m do ponto de teste (-23.55, -46.63).
        point("equipamento_cultura_bibliotecas.1", 333421.2619586424, 7394534.460954394,
              {"nm_equipamento": "Biblioteca Fictícia Centro", "tx_endereco": "Rua Fictícia, 100"}),
        # ~4,5 km.
        point("equipamento_cultura_bibliotecas.2", 336649.8096756011, 7398004.129565908,
              {"nm_equipamento": "Biblioteca Fictícia Norte", "tx_endereco": "Av. Fictícia, 2000"}),
    ]}
    clinics = {"type": "FeatureCollection", "crs": crs, "features": [
        # ~830 m.
        point("equipamento_saude_ambulatorios_especializados.1", 334142.16262234875, 7393988.888766944,
              {"nm_equipamento": "Ambulatório Fictício", "tx_endereco": "Rua Exemplo, 5"}),
    ]}
    for name, fc in [("geosampa_bibliotecas_sample.geojson", libraries),
                     ("geosampa_ambulatorios_sample.geojson", clinics)]:
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as f:
            json.dump(fc, f, ensure_ascii=False, indent=1)


if __name__ == "__main__":
    make_gpkg()
    make_aggregates()
    make_ipvs()
    make_sgb()
    make_geosampa()
    print("fixtures geradas em", HERE)
