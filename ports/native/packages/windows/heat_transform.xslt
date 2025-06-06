<?xml version="1.0" encoding="utf-8"?>
<xsl:stylesheet version="1.0" 
                xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                xmlns:wix="http://wixtoolset.org/schemas/v4/wxs"
                xmlns="http://wixtoolset.org/schemas/v4/wxs"
                exclude-result-prefixes="wix">

  <!-- Identity transform -->
  <xsl:output method="xml" indent="yes" omit-xml-declaration="no" />
  <xsl:strip-space elements="*"/>
  <xsl:template match="@*|node()">
    <xsl:copy>
      <xsl:apply-templates select="@*|node()"/>
    </xsl:copy>
  </xsl:template>

  <!-- Exclude VC redistributable from heat-generated component list -->
  <xsl:template match="wix:Component[contains(wix:File/@Source, 'vc_redist.x64.exe')]" />

  <!-- Exclude obj directory and its contents -->
  <xsl:template match="wix:Directory[@Name='obj']" />
  <xsl:template match="wix:Component[contains(wix:File/@Source, '\obj\')]" />

  <!-- Exclude WiX source files -->
  <xsl:template match="wix:Component[contains(wix:File/@Source, '.wxs')]" />

</xsl:stylesheet>
